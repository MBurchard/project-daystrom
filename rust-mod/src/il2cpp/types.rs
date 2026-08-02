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

/// Opaque handle to IL2CPP type metadata.
#[repr(C)]
pub struct Il2CppType {
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

/// Opaque handle to a resolved field.
#[repr(C)]
pub struct FieldInfo {
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

// ---- Unity value types -----------------------------------------------------

/// Unity `Vector3` value type.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

// ---- IL2CPP collections ----------------------------------------------------

/// Minimal IL2CPP array layout for object/value access through `List<T>`.
#[repr(C)]
pub struct Il2CppArray<T> {
    _object_header: [*const (); 2],
    _bounds: *mut (),
    max_length: usize,
    vector: [T; 0],
}

impl<T: Copy> Il2CppArray<T> {
    /// Read an item from the inline array storage.
    ///
    /// # Safety
    ///
    /// The pointer must reference a valid IL2CPP array and `index` must be within the array's allocated range.
    pub unsafe fn get(&self, index: usize) -> Option<T> {
        if index >= self.max_length {
            return None;
        }
        let ptr = self.vector.as_ptr();
        Some(unsafe { *ptr.add(index) })
    }
}

/// Minimal `System.Collections.Generic.List<T>` layout.
#[repr(C)]
pub struct Il2CppList<T> {
    _object_header: [*const (); 2],
    items: *mut Il2CppArray<T>,
    size: i32,
    _version: i32,
}

impl<T: Copy> Il2CppList<T> {
    /// Number of initialized items.
    pub fn len(&self) -> usize {
        self.size.max(0) as usize
    }

    /// Returns `true` when the list contains no initialized items.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read an initialized list item.
    ///
    /// # Safety
    ///
    /// The list and backing array must be valid IL2CPP objects.
    pub unsafe fn get(&self, index: usize) -> Option<T> {
        if index >= self.len() || self.items.is_null() {
            return None;
        }
        unsafe { (&*self.items).get(index) }
    }
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
    // Accessed via pointer arithmetic in `decode()`.
}

impl Il2CppString {
    /// Convert this IL2CPP string to a Rust `String`.
    ///
    /// Returns `None` if the pointer is null, the length is invalid,
    /// or the UTF-16 data cannot be decoded.
    pub unsafe fn decode(ptr: *const Self) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let il2cpp_str = unsafe { &*ptr };
        if il2cpp_str.length <= 0 {
            return Some(String::new());
        }
        let len = il2cpp_str.length as usize;
        // Chars start right after the length field (offset of length + 4 bytes for the i32)
        let chars_offset = std::mem::offset_of!(Self, length) + size_of::<i32>();
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
        let result = unsafe { Il2CppString::decode(std::ptr::null()) };
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
        let result = unsafe { Il2CppString::decode(&fake as *const _ as *const Il2CppString) };
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
        let result = unsafe { Il2CppString::decode(&fake as *const _ as *const Il2CppString) };
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
        let result = unsafe { Il2CppString::decode(&fake as *const _ as *const Il2CppString) };
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
        let result = unsafe { Il2CppString::decode(&fake as *const _ as *const Il2CppString) };
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn il2cpp_array_reads_items_with_bounds_check() {
        #[repr(C)]
        struct FakeArray {
            header: [*const (); 2],
            bounds: *mut (),
            max_length: usize,
            vector: [i32; 3],
        }

        let fake = FakeArray {
            header: [std::ptr::null(); 2],
            bounds: std::ptr::null_mut(),
            max_length: 3,
            vector: [10, 20, 30],
        };
        let array = unsafe { &*(&fake as *const FakeArray as *const Il2CppArray<i32>) };

        assert_eq!(unsafe { array.get(0) }, Some(10));
        assert_eq!(unsafe { array.get(2) }, Some(30));
        assert_eq!(unsafe { array.get(3) }, None);
    }

    #[test]
    fn il2cpp_list_reads_initialized_items_only() {
        #[repr(C)]
        struct FakeArray {
            header: [*const (); 2],
            bounds: *mut (),
            max_length: usize,
            vector: [i32; 3],
        }

        #[repr(C)]
        struct FakeList {
            header: [*const (); 2],
            items: *mut Il2CppArray<i32>,
            size: i32,
            version: i32,
        }

        let mut fake_array = FakeArray {
            header: [std::ptr::null(); 2],
            bounds: std::ptr::null_mut(),
            max_length: 3,
            vector: [10, 20, 30],
        };
        let fake_list = FakeList {
            header: [std::ptr::null(); 2],
            items: (&mut fake_array as *mut FakeArray).cast::<Il2CppArray<i32>>(),
            size: 2,
            version: 0,
        };
        let list = unsafe { &*(&fake_list as *const FakeList as *const Il2CppList<i32>) };

        assert_eq!(list.len(), 2);
        assert!(!list.is_empty());
        assert_eq!(unsafe { list.get(0) }, Some(10));
        assert_eq!(unsafe { list.get(1) }, Some(20));
        assert_eq!(unsafe { list.get(2) }, None);
    }

    #[test]
    fn il2cpp_list_negative_size_is_empty() {
        #[repr(C)]
        struct FakeList {
            header: [*const (); 2],
            items: *mut Il2CppArray<i32>,
            size: i32,
            version: i32,
        }

        let fake_list = FakeList {
            header: [std::ptr::null(); 2],
            items: std::ptr::null_mut(),
            size: -1,
            version: 0,
        };
        let list = unsafe { &*(&fake_list as *const FakeList as *const Il2CppList<i32>) };

        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(unsafe { list.get(0) }, None);
    }
}
