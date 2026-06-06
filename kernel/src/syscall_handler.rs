use crate::ipc::port::PortManager;
use pulsar_ipc::Message;
use core::slice;
use core::str;

// Em AArch64, o assembly que lida com a exceção de SVC deve extrair
// os registradores x0, x1, x2 e x8 e passá-los para esta função.
#[no_mangle]
pub extern "C" fn handle_syscall(arg1: u64, arg2: u64, arg3: u64, syscall_num: u64) -> i64 {
    match syscall_num {
        // SYS_PORT_CREATE
        1 => {
            let name_str = unsafe { parse_user_string(arg1, arg2) };
            if name_str.is_none() { return -2; } 
            
            match PortManager::create_port(name_str.unwrap()) {
                Ok(port_id) => port_id as i64,
                Err(_) => -1,
            }
        },
        
        // SYS_PORT_CONNECT
        2 => {
            let name_str = unsafe { parse_user_string(arg1, arg2) };
            if name_str.is_none() { return -2; }
            
            match PortManager::connect_port(name_str.unwrap()) {
                Ok(port_id) => port_id as i64,
                Err(_) => -1, 
            }
        },
        
        // SYS_PORT_REQUEST
        3 => {
            let port_id = arg1 as u32;
            let req_ptr = arg2 as *const Message;
            let rep_ptr = arg3 as *mut Message;
            
            let request = unsafe { &*req_ptr };
            let reply = unsafe { &mut *rep_ptr };
            
            match PortManager::send_and_wait(port_id, request, reply) {
                Ok(_) => 0, 
                Err(_) => -3, 
            }
        },
        
        _ => -4, 
    }
}

unsafe fn parse_user_string<'a>(ptr: u64, len: u64) -> Option<&'a str> {
    if ptr == 0 || len == 0 || len > 256 { return None; }
    let slice = slice::from_raw_parts(ptr as *const u8, len as usize);
    str::from_utf8(slice).ok()
}