use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../il2cpp/compatibility_manifest.rs"]
#[allow(dead_code)]
mod compatibility_manifest;

use compatibility_manifest::{FeatureStatus, MemberSpec, SymbolSpec, assess as assess_manifest, matches_names};

#[derive(Debug, Default)]
struct ClassDump {
    base_class: Option<String>,
    methods: Vec<MethodDump>,
    fields: Vec<FieldDump>,
}

#[derive(Debug, Eq, PartialEq)]
struct MethodDump {
    name: String,
    return_type: String,
    parameter_types: Vec<String>,
    is_static: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct FieldDump {
    name: String,
    field_type: String,
}

fn main() {
    let paths = env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("Usage: pnpm check:mod:dump -- <dump-directory-or-dump.cs> [...]");
        std::process::exit(2);
    }

    let mut failed = false;
    for path in paths {
        match check_dump(&path) {
            Ok(true) => {}
            Ok(false) => failed = true,
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                failed = true;
            }
        }
    }

    if failed {
        std::process::exit(1);
    }
}

fn check_dump(path: &Path) -> Result<bool, String> {
    let dump_path = if path.is_dir() { path.join("dump.cs") } else { path.to_path_buf() };
    let source =
        fs::read_to_string(&dump_path).map_err(|error| format!("cannot read {}: {error}", dump_path.display()))?;
    let classes = parse_dump(&source);
    let results = assess_manifest(|symbol| symbol_available(&classes, symbol));

    let summary = results
        .iter()
        .map(|(feature, result)| format!("{feature}={}", result.status()))
        .collect::<Vec<_>>()
        .join(", ");
    println!("{}: {summary}", dump_path.display());

    for (feature, result) in &results {
        for symbol in &result.missing_required {
            eprintln!("  {feature}: required symbol/signature unavailable: {symbol}");
        }
        for symbol in &result.missing_optional {
            eprintln!("  {feature}: optional symbol/signature unavailable: {symbol}");
        }
    }

    Ok(results.values().all(|result| result.status() != FeatureStatus::Disabled))
}

fn symbol_available(classes: &HashMap<(String, String), ClassDump>, symbol: &SymbolSpec) -> bool {
    if !classes.contains_key(&(symbol.namespace.to_string(), symbol.class_name.to_string())) {
        return false;
    }

    match symbol.member {
        MemberSpec::Class => true,
        MemberSpec::Method {
            names,
            match_all,
            return_type,
            parameter_types,
            is_static,
        } => matches_names(names, match_all, |name| {
            find_methods(classes, symbol.namespace, symbol.class_name, name)
                .iter()
                .any(|method| {
                    (return_type == "*" || method.return_type == return_type)
                        && method.parameter_types == parameter_types
                        && method.is_static == is_static
                })
        }),
        MemberSpec::Field { names, match_all, field_type } => matches_names(names, match_all, |name| {
            find_fields(classes, symbol.namespace, symbol.class_name, name)
                .iter()
                .any(|field| field_type == "*" || field.field_type == field_type)
        }),
    }
}

fn find_methods<'a>(
    classes: &'a HashMap<(String, String), ClassDump>,
    namespace: &str,
    class_name: &str,
    method_name: &str,
) -> Vec<&'a MethodDump> {
    find_members(classes, namespace, class_name, |class| {
        class.methods.iter().filter(|method| method.name == method_name).collect()
    })
}

fn find_fields<'a>(
    classes: &'a HashMap<(String, String), ClassDump>,
    namespace: &str,
    class_name: &str,
    field_name: &str,
) -> Vec<&'a FieldDump> {
    find_members(classes, namespace, class_name, |class| {
        class.fields.iter().filter(|field| field.name == field_name).collect()
    })
}

fn find_members<'a, T>(
    classes: &'a HashMap<(String, String), ClassDump>,
    namespace: &str,
    class_name: &str,
    select: impl Fn(&'a ClassDump) -> Vec<&'a T> + Copy,
) -> Vec<&'a T> {
    find_members_inner(classes, namespace, class_name, select, &mut HashSet::new())
}

fn find_members_inner<'a, T>(
    classes: &'a HashMap<(String, String), ClassDump>,
    namespace: &str,
    class_name: &str,
    select: impl Fn(&'a ClassDump) -> Vec<&'a T> + Copy,
    visited: &mut HashSet<(String, String)>,
) -> Vec<&'a T> {
    let key = (namespace.to_string(), class_name.to_string());
    if !visited.insert(key.clone()) {
        return Vec::new();
    }
    let Some(class) = classes.get(&key) else {
        return Vec::new();
    };
    let found = select(class);
    if !found.is_empty() {
        return found;
    }

    let Some(base_name) = class.base_class.as_deref() else {
        return Vec::new();
    };
    let base_is_generic = base_name.contains('<');
    let base_root = generic_base_name(base_name);
    if classes.contains_key(&(namespace.to_string(), base_name.to_string())) {
        return find_members_inner(classes, namespace, base_name, select, visited);
    }

    let matches = classes
        .keys()
        .filter(|(_, name)| generic_base_name(name) == base_root && name.contains('<') == base_is_generic)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        return find_members_inner(classes, &matches[0].0, &matches[0].1, select, visited);
    }
    Vec::new()
}

fn generic_base_name(name: &str) -> &str {
    name.split_once('<').map_or(name, |(base, _)| base)
}

fn parse_dump(source: &str) -> HashMap<(String, String), ClassDump> {
    let mut classes = HashMap::new();
    let mut namespace = String::new();
    let mut current_key: Option<(String, String)> = None;

    for line in source.lines() {
        if let Some(value) = line.strip_prefix("// Namespace: ") {
            namespace = value.to_string();
            current_key = None;
            continue;
        }

        if let Some((class_name, base_class)) = parse_class_declaration(line) {
            let key = (namespace.clone(), class_name);
            classes
                .entry(key.clone())
                .or_insert_with(|| ClassDump { base_class, ..ClassDump::default() });
            current_key = Some(key);
            continue;
        }

        let Some(key) = current_key.as_ref() else {
            continue;
        };
        let Some(class) = classes.get_mut(key) else {
            continue;
        };
        if let Some(method) = parse_method(line) {
            class.methods.push(method);
        } else if let Some(field) = parse_field(line) {
            class.fields.push(field);
        }
    }

    classes
}

fn parse_class_declaration(line: &str) -> Option<(String, Option<String>)> {
    let declaration = line.split_once(" // TypeDefIndex:").map_or(line, |(prefix, _)| prefix);
    let marker = [" class ", " struct ", " interface ", " enum "]
        .into_iter()
        .find(|marker| declaration.contains(marker))?;
    let (_, rest) = declaration.split_once(marker)?;
    let (name, base_class) = match rest.split_once(" : ") {
        Some((name, bases)) => (name.trim(), split_top_level_commas(bases).into_iter().next()),
        None => (rest.trim(), None),
    };
    Some((name.to_string(), base_class))
}

fn parse_method(line: &str) -> Option<MethodDump> {
    let line = line.trim();
    if !line.ends_with("{ }") || line.starts_with("//") {
        return None;
    }
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let prefix = line[..open].trim();
    let (name, return_type) = declared_name_and_type(prefix)?;
    let is_static = prefix.split_whitespace().any(|part| part == "static");
    let parameter_types = split_top_level_commas(&line[open + 1..close])
        .into_iter()
        .filter_map(|parameter| parameter_type(&parameter))
        .collect();
    Some(MethodDump {
        name: name.to_string(),
        return_type: return_type.to_string(),
        parameter_types,
        is_static,
    })
}

fn split_top_level_commas(value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                result.push(value[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(value[start..].trim().to_string());
    result
}

fn parameter_type(parameter: &str) -> Option<String> {
    let parameter = strip_default_value(parameter).trim();
    let split = last_top_level_space(parameter)?;
    Some(parameter[..split].trim().to_string())
}

fn strip_default_value(parameter: &str) -> &str {
    let mut depth = 0i32;
    for (index, character) in parameter.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            '=' if depth == 0 => return parameter[..index].trim(),
            _ => {}
        }
    }
    parameter
}

fn last_top_level_space(value: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut last = None;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            character if character.is_whitespace() && depth == 0 => last = Some(index),
            _ => {}
        }
    }
    last
}

fn declared_name_and_type(declaration: &str) -> Option<(&str, &str)> {
    let name_split = last_top_level_space(declaration)?;
    let name = declaration[name_split..].trim();
    let before_name = declaration[..name_split].trim_end();
    let declared_type = last_top_level_space(before_name).map_or(before_name, |index| before_name[index..].trim());
    (!name.is_empty() && !declared_type.is_empty()).then_some((name, declared_type))
}

fn parse_field(line: &str) -> Option<FieldDump> {
    let line = line.trim();
    if line.starts_with("//") || line.contains("{") || !line.contains(';') {
        return None;
    }
    let declaration = line.split(';').next()?.split('=').next()?.trim();
    let (name, field_type) = declared_name_and_type(declaration)?;
    Some(FieldDump {
        name: name.to_string(),
        field_type: field_type.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_class_methods_fields_and_defaults() {
        let dump = r#"
// Namespace: Demo
public class Child : Parent // TypeDefIndex: 1
{
    private bool _enabled; // 0x10
    public static string Get(string key, string fallback = "") { }
}
"#;
        let classes = parse_dump(dump);
        let class = classes.get(&("Demo".to_string(), "Child".to_string())).unwrap();
        assert_eq!(class.base_class.as_deref(), Some("Parent"));
        assert_eq!(
            class.fields,
            vec![FieldDump {
                name: "_enabled".to_string(),
                field_type: "bool".to_string(),
            }]
        );
        assert_eq!(
            class.methods,
            vec![MethodDump {
                name: "Get".to_string(),
                return_type: "string".to_string(),
                parameter_types: vec!["string".to_string(), "string".to_string()],
                is_static: true,
            }]
        );
    }

    #[test]
    fn splits_nested_generic_parameters() {
        assert_eq!(
            split_top_level_commas("Dictionary<long, int> values, Nullable<Vector3> position"),
            vec!["Dictionary<long, int> values", "Nullable<Vector3> position"]
        );
    }

    #[test]
    fn parses_generic_return_and_field_types() {
        let dump = r#"
// Namespace: Demo
public class GenericMembers // TypeDefIndex: 1
{
    private Dictionary<long, int> _values; // 0x10
    public Dictionary<long, int> Get() { }
}
"#;
        let classes = parse_dump(dump);
        let class = classes.get(&("Demo".to_string(), "GenericMembers".to_string())).unwrap();

        assert_eq!(class.fields[0].field_type, "Dictionary<long, int>");
        assert_eq!(class.methods[0].return_type, "Dictionary<long, int>");
    }

    #[test]
    fn resolves_members_from_generic_base_with_multiple_interfaces() {
        let dump = r#"
// Namespace: Demo
public class GenericParent<TLeft, TRight> // TypeDefIndex: 1
{
    public int GetValue() { }
}
public class Child : GenericParent<long, int>, IDisposable // TypeDefIndex: 2
{
}
"#;
        let classes = parse_dump(dump);
        let child = classes.get(&("Demo".to_string(), "Child".to_string())).unwrap();

        assert_eq!(child.base_class.as_deref(), Some("GenericParent<long, int>"));
        assert_eq!(find_methods(&classes, "Demo", "Child", "GetValue").len(), 1);
    }

    #[test]
    fn cyclic_inheritance_terminates_without_members() {
        let dump = r#"
// Namespace: Demo
public class First : Second // TypeDefIndex: 1
{
}
public class Second : First // TypeDefIndex: 2
{
}
"#;
        let classes = parse_dump(dump);

        assert!(find_methods(&classes, "Demo", "First", "Missing").is_empty());
    }
}
