/// A Rust transliteration of the bacnet-stack whois app.
extern crate bacnet_sys;

use std::time::Instant;

#[no_mangle]
extern "C" fn my_i_am_handler(
    service_request: *mut u8,
    service_len: u16,
    src: *mut bacnet_sys::BACNET_ADDRESS,
) {
    println!("service_len = {}", service_len);
}

fn main() {
    println!(
        "bacnet stack v{}",
        String::from_utf8_lossy(bacnet_sys::BACNET_VERSION_TEXT)
    );

    let mut dest = bacnet_sys::BACNET_ADDRESS::default();
    let target_object_instance_min = -1i32;
    let target_object_instance_max = -1i32;

    let a = bacnet_sys::BACNET_BROADCAST_NETWORK;
    println!("BACNET_BROADCAST_NETWORK={}", a);

    unsafe {
        bacnet_sys::bip_get_broadcast_address(&mut dest as *mut _);
    }

    // Device_Set_Object_Instance_Number(BACNET_MAX_INSTANCE);
    unsafe {
        bacnet_sys::Device_Set_Object_Instance_Number(bacnet_sys::BACNET_MAX_INSTANCE);
    }

    // init_service_handlers()
    unsafe {
        bacnet_sys::Device_Init(std::ptr::null_mut());
        bacnet_sys::apdu_set_unrecognized_service_handler_handler(None);
        bacnet_sys::apdu_set_confirmed_handler(
            bacnet_sys::BACNET_CONFIRMED_SERVICE_SERVICE_CONFIRMED_READ_PROPERTY,
            Some(bacnet_sys::handler_read_property)
        );
        bacnet_sys::apdu_set_unconfirmed_handler(
            bacnet_sys::BACNET_UNCONFIRMED_SERVICE_SERVICE_UNCONFIRMED_I_AM,
            Some(my_i_am_handler),
        );

        //apdu_set_abort_handler(
    }
    //
    //
    unsafe {
        bacnet_sys::address_init();
    }
    //
    unsafe {
        bacnet_sys::dlenv_init();
    }

    let mut src = bacnet_sys::BACNET_ADDRESS::default();
    let mut rx_buf = [0u8; bacnet_sys::MAX_MPDU as usize];
    let timeout = 100; // ms
    unsafe {
        bacnet_sys::Send_WhoIs_To_Network(
            &mut dest as *mut _,
            target_object_instance_min,
            target_object_instance_max,
        );
    }
    let start = Instant::now();
    loop {
        let pdu_len = unsafe {
            bacnet_sys::bip_receive(&mut src as *mut _, &mut rx_buf as *mut _, bacnet_sys::MAX_MPDU as u16, timeout)
        };
        if pdu_len > 0 {
            // process
            unsafe {
                bacnet_sys::npdu_handler(&mut src as *mut _, &mut rx_buf as *mut _, pdu_len);
            }
        }

        if start.elapsed().as_secs() > 3 {
            break;
        }
    }

    // atexit(ethernet_cleanup());
    unsafe {
        bacnet_sys::bip_cleanup();
    }
}
