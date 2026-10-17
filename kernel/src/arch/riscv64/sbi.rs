//! Chamadas SBI (Supervisor Binary Interface) ao OpenSBI.

const EXT_TIME: u64 = 0x5449_4D45;
const EXT_IPI: u64 = 0x0073_5049;
const EXT_RFENCE: u64 = 0x5246_4E43;
const EXT_HSM: u64 = 0x0048_534D;
const EXT_SRST: u64 = 0x5352_5354;

#[inline]
fn call(ext: u64, fid: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    let err: i64;
    unsafe {
        core::arch::asm!("ecall",
            inlateout("a0") a0 as i64 => err,
            inlateout("a1") a1 => _,
            in("a2") a2,
            in("a6") fid,
            in("a7") ext,
            options(nostack));
    }
    err
}

pub fn set_timer(t: u64) {
    call(EXT_TIME, 0, t, 0, 0);
}

pub fn send_ipi(mask: u64, base: u64) {
    call(EXT_IPI, 0, mask, base, 0);
}

/// sfence.vma remoto restrito a um ASID, em todas as harts.
pub fn remote_sfence_asid(asid: u64) {
    // hart_mask_base = -1 => todas as harts
    let err: i64;
    unsafe {
        core::arch::asm!("ecall",
            inlateout("a0") 0i64 => err,
            inlateout("a1") u64::MAX => _,
            in("a2") 0u64, in("a3") 0u64, in("a4") asid,
            in("a6") 2u64, in("a7") EXT_RFENCE,
            options(nostack));
    }
    let _ = err;
}

/// fence.i em todas as harts (codigo novo carregado pelo loader).
pub fn remote_fence_i() {
    call(EXT_RFENCE, 0, 0, u64::MAX, 0);
}

pub fn hart_start(hartid: u64, addr: u64, opaque: u64) -> i64 {
    call(EXT_HSM, 0, hartid, addr, opaque)
}

pub fn system_reset(kind: u64, reason: u64) {
    call(EXT_SRST, 0, kind, reason, 0);
}
