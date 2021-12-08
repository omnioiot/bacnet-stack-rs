#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Mutex;

use failure::Fallible;

pub mod whois;

// We need a global structure here for collecting "target addresses"
lazy_static! {
    /// Global tracking struct for target addresses
    static ref TARGET_ADDRESSES: Mutex<HashMap<u32, TargetDevice>> = Mutex::new(HashMap::new());
}

// A structure for tracking
struct TargetDevice {
    addr: bacnet_sys::BACNET_ADDRESS,
    request_invoke_id: u8,
}

// As I understand the BACnet stack, it works by acting as another BACnet device on the network.
//
// This means that there's not really a
//
// To "connect" to a device, we call address_bind_request(device_id, ..) which adds the device (if
// possible) to the internal address cache. [sidenote: The MAX_ADDRESS_CACHE = 255, which I take to
// mean that we can connect to at most 255 devices].

#[derive(Debug)]
pub struct BACnetDevice {
    pub device_id: u32,
    max_apdu: u32,
    addr: bacnet_sys::BACNET_ADDRESS,
}

pub type ObjectType = bacnet_sys::BACNET_OBJECT_TYPE;
pub type ObjectPropertyId = bacnet_sys::BACNET_PROPERTY_ID;

pub type BACNetValue = ();

impl BACnetDevice {
    pub fn builder() -> BACnetDeviceBuilder {
        BACnetDeviceBuilder::default()
    }

    pub fn connect(&mut self) -> Fallible<()> {
        // Add address
        unsafe {
            bacnet_sys::address_add(self.device_id, bacnet_sys::MAX_APDU, &mut self.addr);
        }
        let mut target_addr = bacnet_sys::BACNET_ADDRESS::default();
        // FIXME(tj): Wait until device is bound, or timeout
        let found = unsafe {
            bacnet_sys::address_bind_request(self.device_id, &mut self.max_apdu, &mut target_addr)
        };
        debug!("found = {}", found);
        if found {
            let mut lock = TARGET_ADDRESSES.lock().unwrap();
            lock.insert(
                self.device_id,
                TargetDevice {
                    addr: target_addr,
                    request_invoke_id: 0,
                },
            );
            Ok(())
        } else {
            Err(format_err!("failed to bind to the device"))
        }
    }

    // Read_Property
    //
    // Only reads the present value (property 85)
    pub fn read_prop_present_value(
        &self,
        object_type: ObjectType,
        object_instance: u32,
    ) -> Fallible<BACNetValue> {
        self.read_prop(
            object_type,
            object_instance,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_PRESENT_VALUE,
        )
    }

    /// Read a property
    ///
    /// We call Send_Read_Property_Request, and wait for a result.
    pub fn read_prop(
        &self,
        object_type: ObjectType,
        object_instance: u32,
        property_id: ObjectPropertyId,
    ) -> Fallible<BACNetValue> {
        const TIMEOUT: u32 = 100;
        let request_invoke_id =
            if let Some(h) = TARGET_ADDRESSES.lock().unwrap().get_mut(&self.device_id) {
                let request_invoke_id = unsafe {
                    bacnet_sys::Send_Read_Property_Request(
                        self.device_id,
                        object_type,
                        object_instance,
                        property_id,
                        bacnet_sys::BACNET_ARRAY_ALL,
                    )
                };
                h.request_invoke_id = request_invoke_id;
                request_invoke_id
            } else {
                bail!("Not connected to device {}", self.device_id)
            };

        let mut src = bacnet_sys::BACNET_ADDRESS::default();
        let mut rx_buf = [0u8; bacnet_sys::MAX_MPDU as usize];
        let start = std::time::Instant::now();
        loop {
            let pdu_len = unsafe {
                bacnet_sys::bip_receive(
                    &mut src,
                    &mut rx_buf as *mut _,
                    bacnet_sys::MAX_MPDU as u16,
                    TIMEOUT,
                )
            };
            if pdu_len > 0 {
                unsafe { bacnet_sys::npdu_handler(&mut src, &mut rx_buf as *mut _, pdu_len) }
            }

            // FIXME(tj): Need to do tsm_invoke_id_free() and tsm_invoke_id_failed() in this loop
            // as well.
            if unsafe { bacnet_sys::tsm_invoke_id_free(request_invoke_id) } {
                break;
            }
            if unsafe { bacnet_sys::tsm_invoke_id_failed(request_invoke_id) } {
                bail!("TSM timeout");
            }

            if start.elapsed().as_secs() > 3 {
                // FIXME(tj): A better timeout here...
                bail!("APDU timeout");
            }
        }

        Ok(())
    }

    pub fn disconnect(&self) {
        todo!()
    }
}

impl Drop for BACnetDevice {
    fn drop(&mut self) {
        // FIXME(tj): Disconnect
        info!("disconneting");
    }
}

// ./bacrp 1025 analog-value 22 present-value --mac 192.168.10.96 --dnet 5 --dadr 14
#[derive(Debug)]
pub struct BACnetDeviceBuilder {
    ip: Ipv4Addr,
    dnet: u16,
    dadr: u8,
    port: u16,
    device_id: u32,
}

impl Default for BACnetDeviceBuilder {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::LOCALHOST,
            dnet: 0,
            dadr: 0,
            port: 0xBAC0,
            device_id: 0,
        }
    }
}

impl BACnetDeviceBuilder {
    pub fn ip(mut self, ip: Ipv4Addr) -> Self {
        self.ip = ip;
        self
    }

    pub fn dnet(mut self, dnet: u16) -> Self {
        self.dnet = dnet;
        self
    }

    pub fn dadr(mut self, dadr: u8) -> Self {
        self.dadr = dadr;
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn build(self) -> BACnetDevice {
        let BACnetDeviceBuilder {
            ip,
            dnet,
            dadr,
            port,
            device_id,
        } = self;
        let mut addr = bacnet_sys::BACNET_ADDRESS::default();
        addr.mac[..4].copy_from_slice(&ip.octets());
        addr.mac[4] = (port >> 8) as u8;
        addr.mac[5] = (port & 0xff) as u8;
        addr.mac_len = 6;
        addr.net = dnet;
        addr.adr[0] = dadr;
        addr.len = 1;

        BACnetDevice {
            device_id,
            max_apdu: 0,
            addr,
        }
    }
}

#[no_mangle]
extern "C" fn my_readprop_ack_handler(
    service_request: *mut u8,
    service_len: u16,
    src: *mut bacnet_sys::BACNET_ADDRESS,
    service_data: *mut bacnet_sys::BACNET_CONFIRMED_SERVICE_ACK_DATA,
) {
    let mut data: bacnet_sys::BACNET_READ_PROPERTY_DATA =
        bacnet_sys::BACNET_READ_PROPERTY_DATA::default();

    // TODO(tj): Find the Target_Address == src and
    // NOTE(tj): Should do address_match(&Target_Address, src), but here we'll just assume that
    // it's the right address
    let mut lock = TARGET_ADDRESSES.lock().unwrap();

    for target in lock.values_mut() {
        if unsafe { bacnet_sys::address_match(&mut target.addr, src) }
            && unsafe { (*service_data).invoke_id } == target.request_invoke_id
        {
            // Decode the data
            let len = unsafe {
                bacnet_sys::rp_ack_decode_service_request(
                    service_request,
                    service_len.into(),
                    &mut data as *mut _,
                )
            };
            if len >= 0 {
                unsafe {
                    bacnet_sys::rp_ack_print_data(&mut data);
                }
            } else {
                println!("<decode failed>");
            }
            target.request_invoke_id = 0;
            break;
        }
    }
}
