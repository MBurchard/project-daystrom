// ---- Opaque IL2CPP handles ------------------------------------------------

/// Opaque handle to the IL2CPP application domain.
#[repr(C)]
pub struct Il2CppDomain {
    _opaque: [u8; 0],
}

/// Opaque handle to a loaded assembly.
#[repr(C)]
pub struct Il2CppAssembly {
    _opaque: [u8; 0],
}

/// Opaque handle to an assembly image (contains type metadata).
#[repr(C)]
pub struct Il2CppImage {
    _opaque: [u8; 0],
}

/// Opaque handle to a resolved class/type.
#[repr(C)]
pub struct Il2CppClass {
    _opaque: [u8; 0],
}

/// Opaque handle to a runtime object instance.
#[repr(C)]
pub struct Il2CppObject {
    _opaque: [u8; 0],
}

/// Opaque handle to an IL2CPP exception.
#[repr(C)]
pub struct Il2CppException {
    _opaque: [u8; 0],
}

// ---- MethodInfo -----------------------------------------------------------

/// IL2CPP method metadata. Only the `method_pointer` field is accessed directly;
/// all other fields are treated as opaque padding.
///
/// The actual struct has many more fields, but we only need the function pointer
/// which is always the first field across all IL2CPP versions.
#[repr(C)]
pub struct MethodInfo {
    /// Raw pointer to the compiled method implementation.
    pub method_pointer: *const (),
}

// ---- IL2CPP String --------------------------------------------------------

/// IL2CPP string object (UTF-16 encoded, length-prefixed).
///
/// Layout: `[Il2CppObject header (2 pointers)] [i32 length] [u16 chars...]`
#[repr(C)]
pub struct Il2CppString {
    _object_header: [*const (); 2],
    /// Number of UTF-16 code units (not bytes).
    pub length: i32,
    // UTF-16 chars follow inline as a flexible array member.
    // Accessed via pointer arithmetic in `to_rust_string()`.
}

impl Il2CppString {
    /// Convert this IL2CPP string to a Rust `String`.
    ///
    /// Returns `None` if the pointer is null, the length is invalid,
    /// or the UTF-16 data cannot be decoded.
    pub unsafe fn to_rust_string(ptr: *const Self) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let il2cpp_str = unsafe { &*ptr };
        if il2cpp_str.length <= 0 {
            return Some(String::new());
        }
        let len = il2cpp_str.length as usize;
        // Chars start right after the length field (offset of length + 4 bytes for the i32)
        let chars_offset = std::mem::offset_of!(Self, length) + std::mem::size_of::<i32>();
        let chars_ptr = unsafe { (ptr as *const u8).add(chars_offset) } as *const u16;
        let slice = unsafe { std::slice::from_raw_parts(chars_ptr, len) };
        String::from_utf16(slice).ok()
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il2cpp_string_null_returns_none() {
        let result = unsafe { Il2CppString::to_rust_string(std::ptr::null()) };
        assert_eq!(result, None);
    }

    #[test]
    fn il2cpp_string_empty() {
        // Simulate an Il2CppString with length 0
        #[repr(C)]
        struct FakeString {
            _header: [*const (); 2],
            length: i32,
        }
        let fake = FakeString {
            _header: [std::ptr::null(); 2],
            length: 0,
        };
        let result = unsafe { Il2CppString::to_rust_string(&fake as *const _ as *const Il2CppString) };
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn il2cpp_string_ascii() {
        // Simulate "Hello" as UTF-16
        #[repr(C)]
        struct FakeString {
            _header: [*const (); 2],
            length: i32,
            chars: [u16; 5],
        }
        let fake = FakeString {
            _header: [std::ptr::null(); 2],
            length: 5,
            chars: [b'H' as u16, b'e' as u16, b'l' as u16, b'l' as u16, b'o' as u16],
        };
        let result = unsafe { Il2CppString::to_rust_string(&fake as *const _ as *const Il2CppString) };
        assert_eq!(result, Some("Hello".to_string()));
    }

    #[test]
    fn il2cpp_string_unicode() {
        // Simulate "Ñäbor" as UTF-16 (player names can contain non-ASCII)
        #[repr(C)]
        struct FakeString {
            _header: [*const (); 2],
            length: i32,
            chars: [u16; 5],
        }
        let fake = FakeString {
            _header: [std::ptr::null(); 2],
            length: 5,
            chars: [0x00D1, 0x00E4, b'b' as u16, b'o' as u16, b'r' as u16], // Ñäbor
        };
        let result = unsafe { Il2CppString::to_rust_string(&fake as *const _ as *const Il2CppString) };
        assert_eq!(result, Some("Ñäbor".to_string()));
    }

    #[test]
    fn il2cpp_string_negative_length() {
        #[repr(C)]
        struct FakeString {
            _header: [*const (); 2],
            length: i32,
        }
        let fake = FakeString {
            _header: [std::ptr::null(); 2],
            length: -1,
        };
        let result = unsafe { Il2CppString::to_rust_string(&fake as *const _ as *const Il2CppString) };
        assert_eq!(result, Some(String::new()));
    }
}
