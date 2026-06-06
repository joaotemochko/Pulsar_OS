#![no_std]
use core::arch::asm;
use pulsar_ipc::{Message, IpcError};

const SYS_PORT_CREATE: u64 = 1;
const SYS_PORT_CONNECT: u64 = 2;
const SYS_PORT_REQUEST: u64 = 3;

// Wrapper interno para disparar a interrupção "SVC #0" do ARM
#[inline(always)]
unsafe fn syscall_3(syscall_num: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let mut ret: i64;
    asm!(
        "svc #0",               // Supervisor Call (Pula para o EL1 - Kernel)
        in("x8") syscall_num,   // Coloca o número da syscall no x8
        inout("x0") arg1 => ret,// Coloca arg1 no x0 e guarda a resposta (ret) do x0
        in("x1") arg2,          // Coloca arg2 no x1
        in("x2") arg3,          // Coloca arg3 no x2
        options(nostack)        // Diz ao compilador que não tocamos no stack
    );
    ret
}

fn parse_syscall_result(result: i64) -> Result<u32, IpcError> {
    match result {
        r if r >= 0 => Ok(r as u32),
        -1 => Err(IpcError::PortNotFound),
        -2 => Err(IpcError::PermissionDenied),
        -3 => Err(IpcError::Timeout),
        _ => Err(IpcError::Unknown),
    }
}

// O resto do código permanece idêntico, pois a abstração isolou o Assembly!

pub fn pulsar_port_create(name: &str) -> Result<u32, IpcError> {
    let ptr = name.as_ptr() as u64;
    let len = name.len() as u64;
    let result = unsafe { syscall_3(SYS_PORT_CREATE, ptr, len, 0) };
    parse_syscall_result(result)
}

pub fn pulsar_port_connect(name: &str) -> Result<u32, IpcError> {
    let ptr = name.as_ptr() as u64;
    let len = name.len() as u64;
    let result = unsafe { syscall_3(SYS_PORT_CONNECT, ptr, len, 0) };
    parse_syscall_result(result)
}

pub fn pulsar_port_request(port_id: u32, request: &Message, reply: &mut Message) -> Result<u32, IpcError> {
    let req_ptr = request as *const Message as u64;
    let rep_ptr = reply as *mut Message as u64;
    let result = unsafe { syscall_3(SYS_PORT_REQUEST, port_id as u64, req_ptr, rep_ptr) };
    parse_syscall_result(result)
}