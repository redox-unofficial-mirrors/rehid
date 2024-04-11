// Adapted from hidapi-rs/examples/dump_descriptors.rs, license MIT

use hidapi::HidApi;
use rehid::report_handler::ReportHandler;
use std::collections::VecDeque;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match HidApi::new() {
        Ok(api) => {
            for device in api.device_list() {
                log::info!(
                    "  {} ({:04X}:{:04X} interface {} path {:?}):",
                    match device.product_string() {
                        Some(s) => s,
                        _ => "<COULD NOT FETCH>",
                    },
                    device.vendor_id(),
                    device.product_id(),
                    device.interface_number(),
                    device.path()
                );

                match (
                    device.vendor_id(),
                    device.product_id(),
                    device.interface_number(),
                ) {
                    // Enable some ID and interface ID to capture
                    // (0xXXXX, 0xXXXX, 0) => {}
                    _ => continue,
                }

                let mut descriptor = vec![0u8; 2048];
                match device.open_device(&api) {
                    Ok(dev) => match dev.get_report_descriptor(&mut descriptor) {
                        Ok(length) => {
                            log::debug!("    Descriptor {:X?}", &mut descriptor[..length]);
                            let mut handler = ReportHandler::new(&descriptor[..length]).unwrap();
                            loop {
                                let mut report = vec![0; handler.total_byte_length];
                                match dev.read(&mut report) {
                                    Ok(length) => {
                                        log::debug!("    Report {:X?}", &mut report[..length]);
                                        for event in handler.handle(&report[..length]).unwrap() {
                                            log::info!("{:X?}", event);
                                        }
                                    }
                                    Err(err) => {
                                        log::error!("    Failed to read report ({:?})", err);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(err) => log::error!("    Failed to retrieve descriptor ({:?})", err),
                    },
                    Err(err) => log::error!("    Failed to open device ({:?})", err),
                }
                break;
            }
        }
        Err(e) => {
            log::error!("Error: {}", e);
        }
    }
}
