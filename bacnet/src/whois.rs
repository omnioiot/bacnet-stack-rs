//! A highlevel interface to bacnet-sys discovery (Who-Is) functionality
//!
//! Design is like a builder with different parameters and retursn

// So the design of the BACnet stack is a little annoying in that we have to drive the subsystem
// forward, continually called bip_receive(). Each device that's discovered is processed by the
// my_i_am_handler, and we need to a global list of discovered devices.
//
// In effect, this library is not thread-safe, so we need to make sure that only one WhoIs client
// is running at a time.

use std::time::{Duration, Instant};
use std::sync::Mutex;

lazy_static! {
    /// A 
    static ref DISCOVERED_DEVICES: Mutex<Vec<BACnetDevice>> = Mutex::new(vec![]);
}

/// A BACnet device that responded with I-Am in response to the Who-Is we sent out.
pub struct BACnetDevice {
    pub device_id: u32,
    pub max_apdu: u32,
    pub vendor_id: u16,
    pub mac_addr: [u8; 6],
}

pub struct WhoIs {
    /// How long to wait until 
    timeout: Duration, // millis
}

// WhoIs::new().timeout(1000).execute()
impl WhoIs {
    pub fn new() -> WhoIs {
        WhoIs::default()
    }

    /// Set the amount of time to wait for I-Am requests to come in (in millis). Default: 3000
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn execute(self) -> Result<Vec<BACnetDevice>, ()> {
        // create an object with a Drop impl that calls bip_cleanup
        whois(self.timeout);

        let devices = if let Ok(mut lock) = DISCOVERED_DEVICES.lock() {
            lock.drain(..).collect()
        } else {
            vec![] // TODO(tj): Err here
        };

        Ok(devices)
    }
}

impl Default for WhoIs {
    fn default() -> Self {
        WhoIs {
            timeout: Duration::from_secs(3),
        }
    }
}

#[no_mangle]
extern "C" fn my_i_am_handler(
    service_request: *mut u8,
    service_len: u16,
    src: *mut bacnet_sys::BACNET_ADDRESS,
) {
    let mut device_id = 0;
    let mut max_apdu = 0;
    let mut segmentation = 0;
    let mut vendor_id = 0;

    let len = unsafe {
        bacnet_sys::iam_decode_service_request(
            service_request,
            &mut device_id,
            &mut max_apdu,
            &mut segmentation,
            &mut vendor_id
        )
    };
    if len == -1 {
        error!("unable to decode I-Am request...");
        return;
    }
    debug!("device_id = {} max_apdu = {} vendor_id = {}", device_id, max_apdu, vendor_id);
    let mac_len = unsafe { (*src).mac_len } as usize;
    let mut mac_addr = [0u8; 6];
    mac_addr[..mac_len].copy_from_slice(unsafe { &(*src).mac[..mac_len] });

    debug!("MAC = {:02X?}", mac_addr);
    if let Ok(mut lock) = DISCOVERED_DEVICES.lock() {
        lock.push(BACnetDevice {
            device_id,
            max_apdu,
            vendor_id,
            mac_addr,
        });
    }
}

fn whois(timeout: Duration) {
    let mut dest = bacnet_sys::BACNET_ADDRESS::default();
    let target_object_instance_min = -1i32; // TODO(tj): parameterize?
    let target_object_instance_max = -1i32; // TODO(tj): parameterize?

    unsafe {
        bacnet_sys::bip_get_broadcast_address(&mut dest as *mut _);
        bacnet_sys::Device_Set_Object_Instance_Number(bacnet_sys::BACNET_MAX_INSTANCE);
        // service handlers
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

        // FIXME(tj): Set error handlers
        // apdu_set_abort_handler(MyAbortHandler);
        // apdu_set_reject_handler(MyRejectHandler);
        bacnet_sys::address_init();
        bacnet_sys::dlenv_init();
    }

    let mut src = bacnet_sys::BACNET_ADDRESS::default();
    let mut rx_buf = [0u8; bacnet_sys::MAX_MPDU as usize];
    let bip_timeout = 100; // ms
    unsafe {
        bacnet_sys::Send_WhoIs_To_Network(
            &mut dest as *mut _,
            target_object_instance_min,
            target_object_instance_max,
        );
    }
    let start = Instant::now();
    let mut i = 0;
    loop {
        let pdu_len = unsafe {
            bacnet_sys::bip_receive(&mut src as *mut _, &mut rx_buf as *mut _, bacnet_sys::MAX_MPDU as u16, bip_timeout)
        };
        if pdu_len > 0 {
            // process
            unsafe {
                bacnet_sys::npdu_handler(&mut src as *mut _, &mut rx_buf as *mut _, pdu_len);
            }
        }

        if start.elapsed() > timeout {
            break;
        }
        i += 1;
    }
    debug!("Looped {} times", i);

    unsafe {
        bacnet_sys::bip_cleanup();
    }
}
