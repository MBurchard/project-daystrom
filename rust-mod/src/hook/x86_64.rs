//! Minimal x86_64 instruction length decoder for inline hook relocation.
//!
//! Decodes instruction boundaries and identifies 32-bit displacements that reference
//! memory relative to RIP. These need fixup when instructions are copied to a trampoline
//! at a different address.

// ---- Opcode property flags ------------------------------------------------

const M: u8 = 0x01; // has ModR/M byte
const I1: u8 = 0x02; // has 1-byte immediate
const I4: u8 = 0x04; // has 4-byte immediate (2-byte with 0x66 prefix)
const R4: u8 = 0x10; // is rel32 branch (CALL/JMP/Jcc) — needs relocation
const XX: u8 = 0x80; // unhandled opcode — decoder returns an error

// ---- One-byte opcode table ------------------------------------------------

/// Properties for each one-byte opcode (index = opcode value).
///
/// Prefixes (REX 0x40-0x4F, 0x66, 0x67, 0xF0, 0xF2, 0xF3, segments) are consumed
/// in the prefix loop and have value 0 here. The 0x0F two-byte escape is also 0
/// because the second byte is looked up in [`OP2`].
#[rustfmt::skip]
const OP1: [u8; 256] = [
    // 0x00 ADD           OR            2-byte escape
    M,    M,    M,    M,    I1,   I4,   XX,   XX,   M,    M,    M,    M,    I1,   I4,   XX,   0,
    // 0x10 ADC           SBB
    M,    M,    M,    M,    I1,   I4,   XX,   XX,   M,    M,    M,    M,    I1,   I4,   XX,   XX,
    // 0x20 AND           SUB           (0x26/0x2E = segment prefixes)
    M,    M,    M,    M,    I1,   I4,   0,    XX,   M,    M,    M,    M,    I1,   I4,   0,    XX,
    // 0x30 XOR           CMP           (0x36/0x3E = segment prefixes)
    M,    M,    M,    M,    I1,   I4,   0,    XX,   M,    M,    M,    M,    I1,   I4,   0,    XX,
    // 0x40 REX prefixes (consumed in prefix loop)
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    // 0x50 PUSH/POP register
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    // 0x60 misc (PUSHA/POPA invalid in 64-bit, MOVSXD, prefixes, PUSH/IMUL imm)
    XX,   XX,   XX,   M,    0,    0,    0,    0,    I4,   M|I4, I1,   M|I1, 0,    0,    0,    0,
    // 0x70 Jcc rel8 — marked XX: short branches need expansion, not simple relocation
    XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,   XX,
    // 0x80 Group1 imm, TEST, XCHG, MOV, LEA, POP
    M|I1, M|I4, XX,   M|I1, M,    M,    M,    M,    M,    M,    M,    M,    M,    M,    M,    M,
    // 0x90 NOP, XCHG, CBW, CWD, CALLF(XX), FWAIT, PUSHF, POPF, SAHF, LAHF
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,    XX,   0,    0,    0,    0,    0,
    // 0xA0 MOV moffs(XX), string ops, TEST AL/rAX
    XX,   XX,   XX,   XX,   0,    0,    0,    0,    I1,   I4,   0,    0,    0,    0,    0,    0,
    // 0xB0 MOV r8,imm8 (×8)    MOV r,imm32 (×8, imm64 with REX.W for B8-BF)
    I1,   I1,   I1,   I1,   I1,   I1,   I1,   I1,   I4,   I4,   I4,   I4,   I4,   I4,   I4,   I4,
    // 0xC0 shifts, RET, VEX(XX), MOV rm/imm, ENTER(XX), LEAVE, RETF(XX), INT
    M|I1, M|I1, XX,   0,    XX,   XX,   M|I1, M|I4, XX,   0,    XX,   0,    0,    I1,   XX,   0,
    // 0xD0 shifts, BCD(XX), XLAT, x87 FPU (all have ModR/M)
    M,    M,    M,    M,    XX,   XX,   XX,   0,    M,    M,    M,    M,    M,    M,    M,    M,
    // 0xE0 LOOP(XX), IN/OUT imm8, CALL/JMP rel32, JMPF(XX), JMP rel8(XX), IN/OUT DX
    XX,   XX,   XX,   XX,   I1,   I1,   I1,   I1,   R4,   R4,   XX,   XX,   0,    0,    0,    0,
    // 0xF0 LOCK, INT1(XX), REP, HLT, CMC, Group3, CLC..STD, Group4/5
    0,    XX,   0,    0,    0,    0,    M,    M,    0,    0,    0,    0,    0,    0,    M,    M,
];

// ---- Two-byte opcode table (0x0F prefix) ----------------------------------

/// Properties for two-byte opcodes (0x0F + index byte).
///
/// Most 0x0F opcodes have a ModR/M byte, so the default is [`M`].
/// Specific overrides handle instructions without ModR/M, with immediates,
/// and conditional branches.
const OP2: [u8; 256] = make_op2();

const fn make_op2() -> [u8; 256] {
    let mut t = [M; 256];

    // No operands
    t[0x05] = 0; // SYSCALL
    t[0x06] = 0; // CLTS
    t[0x07] = 0; // SYSRET
    t[0x08] = 0; // INVD
    t[0x09] = 0; // WBINVD
    t[0x0B] = 0; // UD2
    t[0x30] = 0; // WRMSR
    t[0x31] = 0; // RDTSC
    t[0x32] = 0; // RDMSR
    t[0x33] = 0; // RDPMC
    t[0x34] = 0; // SYSENTER
    t[0x35] = 0; // SYSEXIT
    t[0x77] = 0; // EMMS
    t[0xA0] = 0; // PUSH FS
    t[0xA1] = 0; // POP FS
    t[0xA2] = 0; // CPUID
    t[0xA8] = 0; // PUSH GS
    t[0xA9] = 0; // POP GS

    // BSWAP r32/r64 (register encoded in opcode, no ModR/M)
    t[0xC8] = 0;
    t[0xC9] = 0;
    t[0xCA] = 0;
    t[0xCB] = 0;
    t[0xCC] = 0;
    t[0xCD] = 0;
    t[0xCE] = 0;
    t[0xCF] = 0;

    // Jcc rel32
    t[0x80] = R4;
    t[0x81] = R4;
    t[0x82] = R4;
    t[0x83] = R4;
    t[0x84] = R4;
    t[0x85] = R4;
    t[0x86] = R4;
    t[0x87] = R4;
    t[0x88] = R4;
    t[0x89] = R4;
    t[0x8A] = R4;
    t[0x8B] = R4;
    t[0x8C] = R4;
    t[0x8D] = R4;
    t[0x8E] = R4;
    t[0x8F] = R4;

    // ModR/M + imm8
    t[0x70] = M | I1; // PSHUFD / PSHUFLW / PSHUFHW
    t[0x71] = M | I1; // Group 12 (PSRL/PSRA/PSLL)
    t[0x72] = M | I1; // Group 13
    t[0x73] = M | I1; // Group 14
    t[0xA4] = M | I1; // SHLD r/m, r, imm8
    t[0xAC] = M | I1; // SHRD r/m, r, imm8
    t[0xBA] = M | I1; // Group 8 (BT/BTS/BTR/BTC imm8)
    t[0xC2] = M | I1; // CMPPS/PD/SS/SD
    t[0xC4] = M | I1; // PINSRW
    t[0xC5] = M | I1; // PEXTRW
    t[0xC6] = M | I1; // SHUFPS/PD

    // 3-byte escapes (handled specially in decode, mark XX so we don't mis-decode)
    t[0x38] = XX;
    t[0x3A] = XX;

    t
}

// ---- Decoded instruction --------------------------------------------------

/// Result of decoding a single x86_64 instruction.
pub struct Insn {
    /// Total instruction length in bytes (including all prefixes).
    pub len: usize,
    /// Byte offset of a 32-bit displacement that needs relocation.
    ///
    /// Covers both RIP-relative memory operands (`[RIP+disp32]` via ModR/M) and
    /// relative branch targets (CALL/JMP/Jcc rel32). In both cases the adjustment
    /// formula is the same: `new_disp = old_disp + (old_addr - new_addr)`.
    pub reloc_offset: Option<usize>,
}

// ---- Decoder --------------------------------------------------------------

/// Decode one x86_64 instruction, returning its length and relocation info.
///
/// Returns an error for opcodes that cannot be safely relocated (short branches,
/// exotic encodings, or truly invalid instructions in 64-bit mode).
pub fn decode(code: &[u8]) -> Result<Insn, String> {
    if code.is_empty() {
        return Err("empty code buffer".to_string());
    }

    let mut pos = 0;
    let mut has_66 = false;
    let mut rex_w = false;

    // 1. Legacy prefixes
    loop {
        let Some(&b) = code.get(pos) else {
            return Err("truncated at prefix".to_string());
        };
        match b {
            0x66 => {
                has_66 = true;
                pos += 1;
            }
            0x67 | 0xF0 | 0xF2 | 0xF3 | 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 => {
                pos += 1;
            }
            _ => break,
        }
    }

    // 2. REX prefix (0x40-0x4F)
    if let Some(&b) = code.get(pos) {
        if (0x40..=0x4F).contains(&b) {
            rex_w = b & 0x08 != 0;
            pos += 1;
        }
    }

    // 3. Opcode
    let opcode = *code.get(pos).ok_or("truncated at opcode")?;
    pos += 1;

    let flags = if opcode == 0x0F {
        let op2 = *code.get(pos).ok_or("truncated at 2-byte opcode")?;
        pos += 1;

        // 3-byte escapes: 0F 38 XX (ModR/M), 0F 3A XX (ModR/M + imm8)
        if op2 == 0x38 {
            let _op3 = *code.get(pos).ok_or("truncated at 3-byte opcode")?;
            pos += 1;
            M
        } else if op2 == 0x3A {
            let _op3 = *code.get(pos).ok_or("truncated at 3-byte opcode")?;
            pos += 1;
            M | I1
        } else {
            OP2[op2 as usize]
        }
    } else {
        OP1[opcode as usize]
    };

    if flags & XX != 0 {
        return Err(format!("unhandled opcode {opcode:#04x} at byte {}", pos - 1));
    }

    // 4. ModR/M + optional SIB + displacement
    let mut reloc_offset = None;
    let mut modrm_reg = 0u8;

    if flags & M != 0 {
        let modrm = *code.get(pos).ok_or("truncated at ModR/M")?;
        pos += 1;

        let mod_field = modrm >> 6;
        let rm = modrm & 0x07;
        modrm_reg = (modrm >> 3) & 0x07;

        match mod_field {
            0b00 => {
                if rm == 0b101 {
                    // [RIP + disp32] — this is the one we need to relocate
                    reloc_offset = Some(pos);
                    pos += 4;
                } else if rm == 0b100 {
                    // SIB byte follows
                    let sib = *code.get(pos).ok_or("truncated at SIB")?;
                    pos += 1;
                    if sib & 0x07 == 0b101 {
                        // SIB base = 101 with mod=00 → disp32 (no base register)
                        pos += 4;
                    }
                }
                // Other rm values: register-indirect [reg], no displacement
            }
            0b01 => {
                if rm == 0b100 {
                    pos += 1; // SIB byte
                }
                pos += 1; // disp8
            }
            0b10 => {
                if rm == 0b100 {
                    pos += 1; // SIB byte
                }
                pos += 4; // disp32
            }
            0b11 => {} // register-to-register, no memory operand
            _ => unreachable!(),
        }
    }

    // 5. Immediate operand
    //
    // Group 3 special case (0xF6/0xF7): only the TEST variant (reg=0 or 1)
    // has an immediate. Other variants (NOT, NEG, MUL, DIV, IDIV) do not.
    if opcode == 0xF6 && modrm_reg <= 1 {
        pos += 1; // TEST rm8, imm8
    } else if opcode == 0xF7 && modrm_reg <= 1 {
        pos += if has_66 && !rex_w { 2 } else { 4 }; // TEST rm, imm16/32
    }

    if flags & R4 != 0 {
        // Relative branch (CALL/JMP/Jcc rel32) — always 4 bytes, needs relocation
        reloc_offset = Some(pos);
        pos += 4;
    } else if flags & I4 != 0 {
        // 4-byte immediate (or special cases)
        if rex_w && (0xB8..=0xBF).contains(&opcode) {
            pos += 8; // MOV r64, imm64 — the only 8-byte immediate in x86_64
        } else if has_66 && !rex_w {
            pos += 2; // 0x66 prefix reduces 32-bit immediate to 16-bit
        } else {
            pos += 4;
        }
    } else if flags & I1 != 0 {
        pos += 1;
    }

    if pos > code.len() {
        return Err(format!("instruction extends beyond buffer ({pos} > {})", code.len()));
    }

    Ok(Insn { len: pos, reloc_offset })
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a byte sequence and return the instruction length.
    fn len(bytes: &[u8]) -> usize {
        decode(bytes).expect("decode failed").len
    }

    /// Decode and return the relocation offset (if any).
    fn reloc(bytes: &[u8]) -> Option<usize> {
        decode(bytes).expect("decode failed").reloc_offset
    }

    // -- Basic instructions --

    #[test]
    fn nop() {
        assert_eq!(len(&[0x90]), 1);
        assert_eq!(reloc(&[0x90]), None);
    }

    #[test]
    fn push_rbp() {
        // 0x55 = PUSH RBP (no REX needed, RBP is in base 8 regs)
        assert_eq!(len(&[0x55]), 1);
    }

    #[test]
    fn push_r12() {
        // 0x41 0x54 = REX.B + PUSH R12
        assert_eq!(len(&[0x41, 0x54]), 2);
    }

    #[test]
    fn ret() {
        assert_eq!(len(&[0xC3]), 1);
    }

    // -- MOV / arithmetic with ModR/M --

    #[test]
    fn mov_rbp_rsp() {
        // 48 89 E5 = MOV RBP, RSP (REX.W + MOV r/m64, r64)
        assert_eq!(len(&[0x48, 0x89, 0xE5]), 3);
        assert_eq!(reloc(&[0x48, 0x89, 0xE5]), None);
    }

    #[test]
    fn sub_rsp_imm8() {
        // 48 83 EC 28 = SUB RSP, 0x28 (REX.W + Group1 r/m64, imm8)
        assert_eq!(len(&[0x48, 0x83, 0xEC, 0x28]), 4);
    }

    #[test]
    fn sub_rsp_imm32() {
        // 48 81 EC 00 01 00 00 = SUB RSP, 0x100 (REX.W + Group1 r/m64, imm32)
        assert_eq!(len(&[0x48, 0x81, 0xEC, 0x00, 0x01, 0x00, 0x00]), 7);
    }

    // -- RIP-relative addressing --

    #[test]
    fn lea_rip_relative() {
        // 48 8D 0D 78 56 34 12 = LEA RCX, [RIP+0x12345678]
        let bytes = [0x48, 0x8D, 0x0D, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(len(&bytes), 7);
        assert_eq!(reloc(&bytes), Some(3)); // disp32 starts at byte 3
    }

    #[test]
    fn mov_rip_relative() {
        // 48 8B 05 10 00 00 00 = MOV RAX, [RIP+0x10]
        let bytes = [0x48, 0x8B, 0x05, 0x10, 0x00, 0x00, 0x00];
        assert_eq!(len(&bytes), 7);
        assert_eq!(reloc(&bytes), Some(3));
    }

    #[test]
    fn cmp_rip_relative() {
        // 48 39 05 10 00 00 00 = CMP [RIP+0x10], RAX
        let bytes = [0x48, 0x39, 0x05, 0x10, 0x00, 0x00, 0x00];
        assert_eq!(len(&bytes), 7);
        assert_eq!(reloc(&bytes), Some(3));
    }

    // -- Relative branches --

    #[test]
    fn call_rel32() {
        // E8 78 56 34 12 = CALL +0x12345678
        let bytes = [0xE8, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(len(&bytes), 5);
        assert_eq!(reloc(&bytes), Some(1)); // rel32 starts at byte 1
    }

    #[test]
    fn jmp_rel32() {
        // E9 78 56 34 12 = JMP +0x12345678
        let bytes = [0xE9, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(len(&bytes), 5);
        assert_eq!(reloc(&bytes), Some(1));
    }

    #[test]
    fn jcc_rel32() {
        // 0F 84 78 56 34 12 = JE +0x12345678
        let bytes = [0x0F, 0x84, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(len(&bytes), 6);
        assert_eq!(reloc(&bytes), Some(2));
    }

    // -- Short branches are rejected --

    #[test]
    fn jcc_rel8_rejected() {
        // 74 05 = JE +5 (short branch, needs expansion)
        assert!(decode(&[0x74, 0x05]).is_err());
    }

    #[test]
    fn jmp_rel8_rejected() {
        // EB 05 = JMP +5
        assert!(decode(&[0xEB, 0x05]).is_err());
    }

    // -- MOV with immediate --

    #[test]
    fn mov_r32_imm32() {
        // B8 78 56 34 12 = MOV EAX, 0x12345678
        assert_eq!(len(&[0xB8, 0x78, 0x56, 0x34, 0x12]), 5);
    }

    #[test]
    fn mov_r64_imm64() {
        // 48 B8 01..08 = MOV RAX, imm64 (REX.W + B8)
        let bytes = [0x48, 0xB8, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(len(&bytes), 10);
    }

    // -- SIB addressing --

    #[test]
    fn mov_sib_base_disp8() {
        // 48 8B 44 24 08 = MOV RAX, [RSP+8] (SIB: base=RSP, index=none)
        assert_eq!(len(&[0x48, 0x8B, 0x44, 0x24, 0x08]), 5);
    }

    #[test]
    fn mov_sib_base_disp32() {
        // 48 8B 84 24 00 01 00 00 = MOV RAX, [RSP+0x100]
        assert_eq!(len(&[0x48, 0x8B, 0x84, 0x24, 0x00, 0x01, 0x00, 0x00]), 8);
    }

    // -- Group 3 (TEST special case) --

    #[test]
    fn test_rm8_imm8() {
        // F6 C0 42 = TEST AL, 0x42 (Group 3, reg=0 → has imm8)
        assert_eq!(len(&[0xF6, 0xC0, 0x42]), 3);
    }

    #[test]
    fn test_rm32_imm32() {
        // F7 C0 78 56 34 12 = TEST EAX, 0x12345678 (Group 3, reg=0 → has imm32)
        assert_eq!(len(&[0xF7, 0xC0, 0x78, 0x56, 0x34, 0x12]), 6);
    }

    #[test]
    fn not_rm32() {
        // F7 D0 = NOT EAX (Group 3, reg=2 → no immediate)
        assert_eq!(len(&[0xF7, 0xD0]), 2);
    }

    // -- Multi-byte NOP --

    #[test]
    fn nop_3byte() {
        // 0F 1F 00 = NOP [EAX] (3-byte NOP)
        assert_eq!(len(&[0x0F, 0x1F, 0x00]), 3);
    }

    #[test]
    fn nop_7byte() {
        // 0F 1F 80 00 00 00 00 = NOP [RAX+0] (7-byte NOP)
        assert_eq!(len(&[0x0F, 0x1F, 0x80, 0x00, 0x00, 0x00, 0x00]), 7);
    }

    // -- Operand size prefix --

    #[test]
    fn push_imm16_with_66() {
        // 66 68 34 12 = PUSH 0x1234 (16-bit with 0x66 prefix)
        assert_eq!(len(&[0x66, 0x68, 0x34, 0x12]), 4);
    }

    // -- CMOV (2-byte opcode) --

    #[test]
    fn cmovne() {
        // 0F 45 C1 = CMOVNE EAX, ECX
        assert_eq!(len(&[0x0F, 0x45, 0xC1]), 3);
    }

    // -- REP prefix --

    #[test]
    fn rep_movsb() {
        // F3 A4 = REP MOVSB (prefix + 1-byte opcode, no ModR/M)
        assert_eq!(len(&[0xF3, 0xA4]), 2);
    }

    // -- 3-byte opcodes --

    #[test]
    fn three_byte_0f38() {
        // 66 0F 38 00 C1 = PSHUFB XMM0, XMM1 (3-byte opcode + ModR/M)
        assert_eq!(len(&[0x66, 0x0F, 0x38, 0x00, 0xC1]), 5);
    }

    #[test]
    fn three_byte_0f3a() {
        // 66 0F 3A 0F C1 03 = PALIGNR XMM0, XMM1, 3 (3-byte opcode + ModR/M + imm8)
        assert_eq!(len(&[0x66, 0x0F, 0x3A, 0x0F, 0xC1, 0x03]), 6);
    }

    // -- LOCK prefix --

    #[test]
    fn lock_cmpxchg() {
        // F0 48 0F B1 08 = LOCK CMPXCHG [RAX], RCX
        assert_eq!(len(&[0xF0, 0x48, 0x0F, 0xB1, 0x08]), 5);
    }

    // -- Multiple instructions in sequence --

    #[test]
    fn typical_prologue() {
        // Common MSVC prologue:
        // 48 89 5C 24 08    MOV [RSP+8], RBX       (5 bytes)
        // 48 89 74 24 10    MOV [RSP+0x10], RSI     (5 bytes)
        // 57                PUSH RDI                 (1 byte)
        // 48 83 EC 20       SUB RSP, 0x20            (4 bytes)
        let code: &[u8] = &[
            0x48, 0x89, 0x5C, 0x24, 0x08,
            0x48, 0x89, 0x74, 0x24, 0x10,
            0x57,
            0x48, 0x83, 0xEC, 0x20,
        ];

        let mut offset = 0;
        let mut insns = Vec::new();
        while offset < 14 {
            let insn = decode(&code[offset..]).unwrap();
            offset += insn.len;
            insns.push(insn);
        }

        assert_eq!(insns.len(), 4);
        assert_eq!(insns[0].len, 5);
        assert_eq!(insns[1].len, 5);
        assert_eq!(insns[2].len, 1);
        assert_eq!(insns[3].len, 4);
        assert_eq!(offset, 15);
    }
}
