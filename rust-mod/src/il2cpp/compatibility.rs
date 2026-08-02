use std::collections::BTreeMap;
use std::sync::{Once, OnceLock};

use log::{info, warn};

use super::api::Il2CppApi;
use super::compatibility_manifest::{
    FeatureResult, FeatureStatus, MemberSpec, SymbolSpec, assess as assess_manifest, matches_names,
};
use super::resolver;
use super::types::{Il2CppType, MethodInfo};

const LOG_TARGET: &str = "IL2CPP.Compatibility";
const METHOD_ATTRIBUTE_STATIC: u32 = 0x0010;

const IL2CPP_TYPE_VOID: i32 = 0x01;
const IL2CPP_TYPE_BOOLEAN: i32 = 0x02;
const IL2CPP_TYPE_I4: i32 = 0x08;
const IL2CPP_TYPE_I8: i32 = 0x0a;
const IL2CPP_TYPE_U8: i32 = 0x0b;
const IL2CPP_TYPE_R4: i32 = 0x0c;
const IL2CPP_TYPE_STRING: i32 = 0x0e;
const IL2CPP_TYPE_VALUETYPE: i32 = 0x11;
const IL2CPP_TYPE_CLASS: i32 = 0x12;
const IL2CPP_TYPE_ARRAY: i32 = 0x14;
const IL2CPP_TYPE_GENERICINST: i32 = 0x15;
const IL2CPP_TYPE_OBJECT: i32 = 0x1c;
const IL2CPP_TYPE_SZARRAY: i32 = 0x1d;
const IL2CPP_TYPE_ENUM: i32 = 0x55;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AbiKind {
    Any,
    Void,
    Boolean,
    I32,
    I64,
    U64,
    F32,
    String,
    Reference,
    ValueType,
}

#[derive(Debug)]
pub struct CompatibilityReport {
    features: BTreeMap<&'static str, FeatureResult>,
}

impl CompatibilityReport {
    pub fn status(&self, feature: &str) -> FeatureStatus {
        self.features
            .get(feature)
            .map(FeatureResult::status)
            .unwrap_or(FeatureStatus::Disabled)
    }

    pub fn is_enabled(&self, feature: &str) -> bool {
        self.status(feature) != FeatureStatus::Disabled
    }

    fn log(&self) {
        let summary = self
            .features
            .iter()
            .map(|(feature, result)| format!("{feature}={}", result.status()))
            .collect::<Vec<_>>()
            .join(", ");
        info!(target: LOG_TARGET, "Compatibility: {summary}");

        for (feature, result) in &self.features {
            for symbol in &result.missing_required {
                warn!(target: LOG_TARGET, "{feature}: required symbol unavailable: {symbol}");
            }
            for symbol in &result.missing_optional {
                warn!(target: LOG_TARGET, "{feature}: optional symbol unavailable: {symbol}");
            }
        }
    }
}

static REPORT: OnceLock<CompatibilityReport> = OnceLock::new();
static UNINITIALIZED_WARNING: Once = Once::new();

/// Run the compatibility preflight once and retain the result for nested feature installers.
pub fn initialize(api: &Il2CppApi) -> &'static CompatibilityReport {
    REPORT.get_or_init(|| {
        let report = CompatibilityReport {
            features: assess_manifest(|symbol| symbol_available(api, symbol)),
        };
        report.log();
        report
    })
}

/// Return whether a feature passed its required runtime checks.
///
/// Returns `false` when the compatibility preflight has not run yet.
pub fn is_enabled(feature: &str) -> bool {
    let Some(report) = REPORT.get() else {
        UNINITIALIZED_WARNING.call_once(|| {
            warn!(target: LOG_TARGET, "Compatibility queried before initialization; disabling guarded features");
        });
        return false;
    };
    report.is_enabled(feature)
}

fn symbol_available(api: &Il2CppApi, symbol: &SymbolSpec) -> bool {
    let Some(class) = symbol
        .assemblies
        .iter()
        .find_map(|assembly| resolver::try_resolve_class(api, assembly, symbol.namespace, symbol.class_name))
    else {
        return false;
    };

    match symbol.member {
        MemberSpec::Class => true,
        MemberSpec::Method {
            names,
            match_all,
            return_type,
            parameter_types,
            is_static,
        } => matches_names(names, match_all, |name| {
            let Some(method) = resolver::try_resolve_method(api, class, name, parameter_types.len() as i32) else {
                return false;
            };
            method_abi_matches(api, method, return_type, parameter_types, is_static)
        }),
        MemberSpec::Field { names, match_all, .. } => {
            matches_names(names, match_all, |name| resolver::has_field(api, class, name))
        }
    }
}

fn method_abi_matches(
    api: &Il2CppApi,
    method: *const MethodInfo,
    return_type: &str,
    parameter_types: &[&str],
    is_static: bool,
) -> bool {
    let mut implementation_flags = 0;
    let method_flags = unsafe { (api.method_get_flags)(method, &mut implementation_flags) };
    let actual_is_static = method_flags & METHOD_ATTRIBUTE_STATIC != 0;
    if actual_is_static != is_static {
        return false;
    }

    let actual_return_type = unsafe { (api.method_get_return_type)(method) };
    if !abi_type_matches(api, actual_return_type, return_type) {
        return false;
    }

    parameter_types.iter().enumerate().all(|(index, expected)| {
        let actual = unsafe { (api.method_get_param)(method, index as u32) };
        abi_type_matches(api, actual, expected)
    })
}

fn abi_type_matches(api: &Il2CppApi, actual: *const Il2CppType, expected: &str) -> bool {
    let Some(expected_kind) = expected_abi_kind(expected) else {
        return false;
    };
    if expected_kind == AbiKind::Any {
        return true;
    }
    if actual.is_null() {
        return false;
    }

    actual_abi_kind(api, actual).is_some_and(|actual_kind| actual_kind == expected_kind)
}

fn actual_abi_kind(api: &Il2CppApi, actual: *const Il2CppType) -> Option<AbiKind> {
    let type_code = unsafe { (api.type_get_type)(actual) };
    if type_code != IL2CPP_TYPE_GENERICINST {
        return abi_kind_from_type_code(type_code, None);
    }

    let class = unsafe { (api.class_from_type)(actual) };
    if class.is_null() {
        return None;
    }
    abi_kind_from_type_code(type_code, Some(unsafe { (api.class_is_valuetype)(class) }))
}

fn abi_kind_from_type_code(type_code: i32, generic_is_value_type: Option<bool>) -> Option<AbiKind> {
    match type_code {
        IL2CPP_TYPE_VOID => Some(AbiKind::Void),
        IL2CPP_TYPE_BOOLEAN => Some(AbiKind::Boolean),
        IL2CPP_TYPE_I4 => Some(AbiKind::I32),
        IL2CPP_TYPE_I8 => Some(AbiKind::I64),
        IL2CPP_TYPE_U8 => Some(AbiKind::U64),
        IL2CPP_TYPE_R4 => Some(AbiKind::F32),
        IL2CPP_TYPE_STRING => Some(AbiKind::String),
        IL2CPP_TYPE_CLASS | IL2CPP_TYPE_ARRAY | IL2CPP_TYPE_OBJECT | IL2CPP_TYPE_SZARRAY => Some(AbiKind::Reference),
        IL2CPP_TYPE_VALUETYPE | IL2CPP_TYPE_ENUM => Some(AbiKind::ValueType),
        IL2CPP_TYPE_GENERICINST => {
            generic_is_value_type.map(
                |is_value_type| {
                    if is_value_type { AbiKind::ValueType } else { AbiKind::Reference }
                },
            )
        }
        _ => None,
    }
}

fn expected_abi_kind(type_name: &str) -> Option<AbiKind> {
    match type_name {
        "*" => Some(AbiKind::Any),
        "void" => Some(AbiKind::Void),
        "bool" => Some(AbiKind::Boolean),
        "int" => Some(AbiKind::I32),
        "long" => Some(AbiKind::I64),
        "ulong" => Some(AbiKind::U64),
        "float" => Some(AbiKind::F32),
        "string" => Some(AbiKind::String),
        "ChangeViewData"
        | "FleetDeployedData"
        | "FleetPlayerData"
        | "HullSpec"
        | "List<CourseData>"
        | "List<FleetDeployedData>"
        | "Message"
        | "NodeAddress"
        | "Toast"
        | "UserProfile" => Some(AbiKind::Reference),
        "DeployedFleetState"
        | "DeployedFleetType"
        | "FleetState"
        | "HullType"
        | "InputInteractionType"
        | "KeyCode"
        | "NodeDepth"
        | "Nullable<ZoomLevels>"
        | "Vector3" => Some(AbiKind::ValueType),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternative_names_need_one_match() {
        assert!(matches_names(&["old", "new"], false, |name| name == "new"));
        assert!(!matches_names(&["old", "new"], false, |_| false));
    }

    #[test]
    fn grouped_names_need_every_match() {
        assert!(matches_names(&["one", "two"], true, |_| true));
        assert!(!matches_names(&["one", "two"], true, |name| name == "one"));
    }

    #[test]
    fn all_manifest_method_types_have_an_abi_classification() {
        for symbol in super::super::compatibility_manifest::SYMBOLS {
            let MemberSpec::Method { return_type, parameter_types, .. } = symbol.member else {
                continue;
            };
            assert!(
                expected_abi_kind(return_type).is_some(),
                "unclassified return type: {return_type}"
            );
            for parameter_type in parameter_types {
                assert!(
                    expected_abi_kind(parameter_type).is_some(),
                    "unclassified parameter type: {parameter_type}"
                );
            }
        }
    }

    #[test]
    fn abi_classification_distinguishes_integer_width_and_storage() {
        assert_ne!(expected_abi_kind("int"), expected_abi_kind("long"));
        assert_ne!(expected_abi_kind("HullSpec"), expected_abi_kind("HullType"));
        assert_eq!(expected_abi_kind("unknown"), None);
    }

    #[test]
    fn il2cpp_type_codes_map_to_abi_categories() {
        assert_eq!(abi_kind_from_type_code(IL2CPP_TYPE_I4, None), Some(AbiKind::I32));
        assert_eq!(abi_kind_from_type_code(IL2CPP_TYPE_I8, None), Some(AbiKind::I64));
        assert_eq!(
            abi_kind_from_type_code(IL2CPP_TYPE_GENERICINST, Some(false)),
            Some(AbiKind::Reference)
        );
        assert_eq!(
            abi_kind_from_type_code(IL2CPP_TYPE_GENERICINST, Some(true)),
            Some(AbiKind::ValueType)
        );
        assert_eq!(abi_kind_from_type_code(IL2CPP_TYPE_GENERICINST, None), None);
        assert_eq!(abi_kind_from_type_code(0x7f, None), None);
    }
}
