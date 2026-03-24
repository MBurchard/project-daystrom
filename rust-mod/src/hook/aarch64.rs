/// ARM64 instruction decoder for inline hook relocation.
///
/// Identifies PC-relative instructions that need fixup when copied from their original
/// location to a trampoline at a different address. Only instructions that appear in
/// typical function prologues are relevant; all others are position-independent.
/// A decoded ARM64 instruction (always 4 bytes).
pub struct Insn {
    /// Type of PC-relative reference that needs relocation, if any.
    pub reloc: Option<Reloc>,
}

/// Describes how a PC-relative instruction encodes its displacement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reloc {
    /// `ADRP Xn, #page` - 21-bit signed immediate, shifted left by 12. Range: ±4GB.
    /// Target = `(PC & !0xFFF) + (imm21 << 12)`.
    Adrp,
    /// `ADR Xn, #offset` - 21-bit signed immediate, no shift. Range: ±1MB.
    /// Target = `PC + imm21`.
    Adr,
    /// `B` / `BL` - 26-bit signed immediate, shifted left by 2. Range: ±128MB.
    Branch26,
    /// `B.cond` / `CBZ` / `CBNZ` / `LDR (literal)` - 19-bit signed immediate, <<2. Range: ±1MB.
    Imm19,
    /// `TBZ` / `TBNZ` - 14-bit signed immediate, shifted left by 2. Range: ±32KB.
    Imm14,
}

/// Decode a single ARM64 instruction and determine whether it needs relocation.
pub fn decode(insn: u32) -> Insn {
    let reloc = if insn & 0x9F00_0000 == 0x9000_0000 {
        // ADRP Xn, #page
        Some(Reloc::Adrp)
    } else if insn & 0x9F00_0000 == 0x1000_0000 {
        // ADR Xn, #offset
        Some(Reloc::Adr)
    } else if insn & 0xFC00_0000 == 0x1400_0000 || insn & 0xFC00_0000 == 0x9400_0000 {
        // B #offset / BL #offset
        Some(Reloc::Branch26)
    } else if insn & 0xFF00_0010 == 0x5400_0000 {
        // B.cond #offset
        Some(Reloc::Imm19)
    } else if insn & 0x7F00_0000 == 0x3400_0000 || insn & 0x7F00_0000 == 0x3500_0000 {
        // CBZ / CBNZ
        Some(Reloc::Imm19)
    } else if insn & 0x3B00_0000 == 0x1800_0000 {
        // LDR (literal) - covers 32-bit, 64-bit, and SIMD variants
        Some(Reloc::Imm19)
    } else if insn & 0x7F00_0000 == 0x3600_0000 || insn & 0x7F00_0000 == 0x3700_0000 {
        // TBZ / TBNZ
        Some(Reloc::Imm14)
    } else {
        None
    };
    Insn { reloc }
}

/// Relocate a PC-relative instruction from `old_pc` to `new_pc`.
///
/// Computes the absolute target from the original instruction and PC, then re-encodes
/// the immediate for the new PC location. Returns `Err` if the new displacement
/// exceeds the instruction's range.
pub fn relocate(insn: u32, reloc: &Reloc, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    match reloc {
        Reloc::Adrp => relocate_adrp(insn, old_pc, new_pc),
        Reloc::Adr => relocate_adr(insn, old_pc, new_pc),
        Reloc::Branch26 => relocate_branch26(insn, old_pc, new_pc),
        Reloc::Imm19 => relocate_imm19(insn, old_pc, new_pc),
        Reloc::Imm14 => relocate_imm14(insn, old_pc, new_pc),
    }
}

// ---- ADRP (page-relative, 21-bit signed, <<12) -----------------------------

/// Extract the 21-bit signed immediate from an ADRP/ADR instruction.
///
/// Encoding: `immlo` at bits 30:29, `immhi` at bits 23:5.
fn extract_imm21(insn: u32) -> i32 {
    let immhi = (insn >> 5) & 0x7FFFF; // bits 23:5
    let immlo = (insn >> 29) & 0x3;    // bits 30:29
    let imm21 = (immhi << 2) | immlo;
    // Sign-extend from 21 bits
    ((imm21 as i32) << 11) >> 11
}

/// Encode a 21-bit signed immediate into an ADRP/ADR instruction.
fn encode_imm21(insn: u32, imm21: i32) -> u32 {
    let val = imm21 as u32 & 0x1F_FFFF;
    let immlo = val & 0x3;
    let immhi = (val >> 2) & 0x7FFFF;
    (insn & 0x9F00_001F) | (immhi << 5) | (immlo << 29)
}

/// Relocate an `ADRP` instruction. Target = `(PC & !0xFFF) + (imm21 << 12)`.
fn relocate_adrp(insn: u32, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    let old_page = old_pc & !0xFFF;
    let new_page = new_pc & !0xFFF;
    let imm21 = extract_imm21(insn);
    let abs_page = old_page.wrapping_add((imm21 as i64 as u64) << 12);
    let new_offset = abs_page.wrapping_sub(new_page) as i64;
    let new_imm21 = new_offset >> 12;

    if !(-0x10_0000_i64..=0xF_FFFF).contains(&new_imm21) {
        return Err(format!(
            "ADRP relocation overflow: delta {new_offset:#x} exceeds ±4GB"
        ));
    }
    Ok(encode_imm21(insn, new_imm21 as i32))
}

// ---- ADR (PC-relative, 21-bit signed, no shift) ----------------------------

/// Relocate an `ADR` instruction. Target = `PC + imm21`.
fn relocate_adr(insn: u32, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    let imm21 = extract_imm21(insn);
    let abs_target = old_pc.wrapping_add(imm21 as i64 as u64);
    let new_offset = abs_target.wrapping_sub(new_pc) as i64;

    if !(-0x10_0000_i64..=0xF_FFFF).contains(&new_offset) {
        return Err(format!(
            "ADR relocation overflow: delta {new_offset:#x} exceeds ±1MB"
        ));
    }
    Ok(encode_imm21(insn, new_offset as i32))
}

// ---- B / BL (26-bit signed, <<2) -------------------------------------------

/// Relocate a `B` or `BL` instruction. Target = `PC + (imm26 << 2)`.
fn relocate_branch26(insn: u32, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    let imm26 = (insn & 0x03FF_FFFF) as i32;
    // Sign-extend from 26 bits
    let imm26 = (imm26 << 6) >> 6;
    let abs_target = old_pc.wrapping_add((imm26 as i64 as u64) << 2);
    let new_offset = abs_target.wrapping_sub(new_pc) as i64;
    let new_imm26 = new_offset >> 2;

    if !(-0x200_0000_i64..=0x1FF_FFFF).contains(&new_imm26) {
        return Err(format!(
            "B/BL relocation overflow: delta {new_offset:#x} exceeds ±128MB"
        ));
    }
    Ok((insn & 0xFC00_0000) | (new_imm26 as u32 & 0x03FF_FFFF))
}

// ---- Imm19 (B.cond, CBZ, CBNZ, LDR literal) -------------------------------

/// Relocate an instruction with a 19-bit signed immediate (<<2).
fn relocate_imm19(insn: u32, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    let imm19 = ((insn >> 5) & 0x7FFFF) as i32;
    // Sign-extend from 19 bits
    let imm19 = (imm19 << 13) >> 13;
    let abs_target = old_pc.wrapping_add((imm19 as i64 as u64) << 2);
    let new_offset = abs_target.wrapping_sub(new_pc) as i64;
    let new_imm19 = new_offset >> 2;

    if !(-0x4_0000_i64..=0x3FFFF).contains(&new_imm19) {
        return Err(format!(
            "imm19 relocation overflow: delta {new_offset:#x} exceeds ±1MB"
        ));
    }
    Ok((insn & 0xFF00_001F) | ((new_imm19 as u32 & 0x7FFFF) << 5))
}

// ---- Imm14 (TBZ, TBNZ) ----------------------------------------------------

/// Relocate an instruction with a 14-bit signed immediate (<<2).
fn relocate_imm14(insn: u32, old_pc: u64, new_pc: u64) -> Result<u32, String> {
    let imm14 = ((insn >> 5) & 0x3FFF) as i32;
    // Sign-extend from 14 bits
    let imm14 = (imm14 << 18) >> 18;
    let abs_target = old_pc.wrapping_add((imm14 as i64 as u64) << 2);
    let new_offset = abs_target.wrapping_sub(new_pc) as i64;
    let new_imm14 = new_offset >> 2;

    if !(-0x2000_i64..=0x1FFF).contains(&new_imm14) {
        return Err(format!(
            "imm14 relocation overflow: delta {new_offset:#x} exceeds ±32KB"
        ));
    }
    Ok((insn & 0xFFF8_001F) | ((new_imm14 as u32 & 0x3FFF) << 5))
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- decode: detection ---------------------------------------------------

    #[test]
    fn decode_adrp() {
        // ADRP X8, #0 → 0x90000008
        let insn = decode(0x9000_0008);
        assert_eq!(insn.reloc, Some(Reloc::Adrp));
    }

    #[test]
    fn decode_adr() {
        // ADR X0, #4 → 0x10000020
        let insn = decode(0x1000_0020);
        assert_eq!(insn.reloc, Some(Reloc::Adr));
    }

    #[test]
    fn decode_b() {
        // B #0x10 → 0x14000004
        let insn = decode(0x1400_0004);
        assert_eq!(insn.reloc, Some(Reloc::Branch26));
    }

    #[test]
    fn decode_bl() {
        // BL #0x10 → 0x94000004
        let insn = decode(0x9400_0004);
        assert_eq!(insn.reloc, Some(Reloc::Branch26));
    }

    #[test]
    fn decode_b_cond() {
        // B.EQ #0x10 → 0x54000080
        let insn = decode(0x5400_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm19));
    }

    #[test]
    fn decode_cbz() {
        // CBZ X0, #0x10 → 0xB4000080
        let insn = decode(0xB400_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm19));
    }

    #[test]
    fn decode_cbnz() {
        // CBNZ X0, #0x10 → 0xB5000080
        let insn = decode(0xB500_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm19));
    }

    #[test]
    fn decode_ldr_literal() {
        // LDR X0, #0x10 → 0x58000080
        let insn = decode(0x5800_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm19));
    }

    #[test]
    fn decode_tbz() {
        // TBZ X0, #0, #0x10 → 0x36000080
        let insn = decode(0x3600_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm14));
    }

    #[test]
    fn decode_tbnz() {
        // TBNZ X0, #0, #0x10 → 0x37000080
        let insn = decode(0x3700_0080);
        assert_eq!(insn.reloc, Some(Reloc::Imm14));
    }

    #[test]
    fn decode_non_relocatable() {
        // STP X29, X30, [SP, #-16]! → 0xA9BF7BFD
        assert!(decode(0xA9BF_7BFD).reloc.is_none());
        // SUB SP, SP, #0x20 → 0xD10083FF
        assert!(decode(0xD100_83FF).reloc.is_none());
        // MOV X0, X1 → 0xAA0103E0
        assert!(decode(0xAA01_03E0).reloc.is_none());
        // RET → 0xD65F03C0
        assert!(decode(0xD65F_03C0).reloc.is_none());
    }

    // -- relocate: ADRP -----------------------------------------------------

    #[test]
    fn relocate_adrp_same_page() {
        // ADRP X8, #0 at PC=0x1000. Trampoline at 0x2000 (same page distance).
        let insn = 0x9000_0008; // ADRP X8, #0
        let result = relocate(insn, &Reloc::Adrp, 0x1000, 0x2000).unwrap();
        // Original target page: 0x1000 & !0xFFF = 0x1000
        // New PC page: 0x2000 & !0xFFF = 0x2000
        // New imm21 = (0x1000 - 0x2000) >> 12 = -1
        let new_imm = extract_imm21(result);
        assert_eq!(new_imm, -1);
        // Rd should be preserved
        assert_eq!(result & 0x1F, 0x08);
    }

    #[test]
    fn relocate_adrp_forward() {
        // ADRP X8, #0x10000 (imm21=16) at PC=0x100000.
        // Target page = 0x100000 + 16*4096 = 0x110000.
        // Trampoline at 0x200000. New imm21 = (0x110000 - 0x200000) >> 12 = -240.
        let insn = encode_imm21(0x9000_0008, 16);
        let result = relocate(insn, &Reloc::Adrp, 0x10_0000, 0x20_0000).unwrap();
        let new_imm = extract_imm21(result);
        assert_eq!(new_imm, -240);
    }

    // -- relocate: ADR ------------------------------------------------------

    #[test]
    fn relocate_adr_basic() {
        // ADR X0, #100 at PC=0x1000. Trampoline at 0x5000.
        let insn = encode_imm21(0x1000_0000, 100);
        let result = relocate(insn, &Reloc::Adr, 0x1000, 0x5000).unwrap();
        // Target = 0x1000 + 100 = 0x1064. New offset = 0x1064 - 0x5000 = -0x3F9C.
        let new_imm = extract_imm21(result);
        assert_eq!(new_imm, -0x3F9C_i32);
    }

    // -- relocate: B/BL -----------------------------------------------------

    #[test]
    fn relocate_branch26_forward() {
        // B #0x100 (imm26=64) at PC=0x1000. Trampoline at 0x2000.
        let insn = 0x1400_0000 | 64; // B #256
        let result = relocate(insn, &Reloc::Branch26, 0x1000, 0x2000).unwrap();
        // Target = 0x1000 + 64*4 = 0x1100. New offset = 0x1100 - 0x2000 = -0xF00.
        let new_imm26 = (result & 0x03FF_FFFF) as i32;
        let new_imm26 = (new_imm26 << 6) >> 6; // sign-extend
        assert_eq!(new_imm26 << 2, -0xF00);
    }

    // -- relocate: imm19 ----------------------------------------------------

    #[test]
    fn relocate_imm19_cbz() {
        // CBZ X0, #0x40 (imm19=16) at PC=0x1000. Trampoline at 0x3000.
        let insn = 0xB400_0000 | (16 << 5); // CBZ X0, #64
        let result = relocate(insn, &Reloc::Imm19, 0x1000, 0x3000).unwrap();
        // Target = 0x1000 + 16*4 = 0x1040. New offset = 0x1040 - 0x3000 = -0x1FC0.
        let new_imm19 = ((result >> 5) & 0x7FFFF) as i32;
        let new_imm19 = (new_imm19 << 13) >> 13;
        assert_eq!(new_imm19 << 2, -0x1FC0);
    }

    // -- relocate: imm14 ----------------------------------------------------

    #[test]
    fn relocate_imm14_tbz() {
        // TBZ X0, #0, #0x20 (imm14=8) at PC=0x1000. Trampoline at 0x1100.
        let insn = 0x3600_0000 | (8 << 5); // TBZ X0, #0, #32
        let result = relocate(insn, &Reloc::Imm14, 0x1000, 0x1100).unwrap();
        // Target = 0x1000 + 8*4 = 0x1020. New offset = 0x1020 - 0x1100 = -0xE0.
        let new_imm14 = ((result >> 5) & 0x3FFF) as i32;
        let new_imm14 = (new_imm14 << 18) >> 18;
        assert_eq!(new_imm14 << 2, -0xE0);
    }

    // -- relocate: overflow -------------------------------------------------

    #[test]
    fn relocate_imm14_overflow() {
        // TBZ with trampoline very far away (>32KB)
        let insn = 0x3600_0000 | (1 << 5);
        let result = relocate(insn, &Reloc::Imm14, 0x1000, 0x100_0000);
        assert!(result.is_err());
    }

    // -- round-trip: encode then extract ------------------------------------

    #[test]
    fn imm21_round_trip() {
        for val in [-1048576, -1, 0, 1, 42, 1048575] {
            let encoded = encode_imm21(0x9000_0000, val);
            let decoded = extract_imm21(encoded);
            assert_eq!(decoded, val, "round-trip failed for {val}");
        }
    }
}
