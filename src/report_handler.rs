use std::collections::{HashMap, HashSet};
use std::error::Error;

use crate::hidreport::{Field, Report, ReportDescriptor};

#[derive(Debug)]
pub struct ReportEvent {
    pub usage_page: u16,
    pub usage: u16,
    pub value: i64,
    pub relative: bool,
}

pub struct ReportHandler {
    pub descriptor: ReportDescriptor,
    pub total_byte_length: usize,
    pub absolutes: HashMap<(u16, u16), i64>,
    pub arrays: HashSet<(u16, u16)>,
}

impl ReportHandler {
    pub fn new(descriptor_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let descriptor = ReportDescriptor::try_from(descriptor_bytes)?;
        let mut total_byte_length = 0;
        for report in descriptor.input_reports() {
            let size = report.size_in_bytes();
            if size > total_byte_length {
                total_byte_length = size;
            }
        }
        Ok(Self {
            descriptor,
            total_byte_length,
            absolutes: HashMap::new(),
            arrays: HashSet::new(),
        })
    }

    pub fn handle(&mut self, report_bytes: &[u8]) -> Result<Vec<ReportEvent>, Box<dyn Error>> {
        let mut events = Vec::new();
        let mut new_arrays = HashSet::new();
        if let Some(report) = self.descriptor.find_input_report(report_bytes) {
            for field in report.fields() {
                match field {
                    Field::Variable(variable) => {
                        let usage_page = variable.usage.usage_page.into();
                        let usage = variable.usage.usage_id.into();
                        let value = if variable.is_signed() {
                            i64::from(variable.extract_i32(&report_bytes)?)
                        } else {
                            i64::from(variable.extract_u32(&report_bytes)?)
                        };
                        let relative = variable.is_relative;
                        log::trace!(
                            "{:?}:{:?} = {:?} ({})",
                            usage_page,
                            usage,
                            value,
                            if relative { "relative" } else { "absolute" }
                        );
                        if relative {
                            if value == 0 {
                                // Skip relative events where value has not changed
                                continue;
                            }
                        } else {
                            if let Some(last_value) = self.absolutes.get(&(usage_page, usage)) {
                                if &value == last_value {
                                    // Skip absolute events where value has not changed
                                    continue;
                                }
                            }
                            // Insert new value
                            self.absolutes.insert((usage_page, usage), value);
                        }

                        events.push(ReportEvent {
                            usage_page,
                            usage,
                            value,
                            relative,
                        });
                    }
                    Field::Array(array) => {
                        log::trace!("{:?}", array);
                        //TODO: use array.is_signed?
                        for index in array.extract_u32(&report_bytes)? {
                            log::trace!("  = {:?}", index);
                            if let Some(usage) = array.usages().get(usize::try_from(index)?) {
                                new_arrays.insert((
                                    u16::from(usage.usage_page),
                                    u16::from(usage.usage_id),
                                ));
                            }
                        }
                    }
                    Field::Constant(_constant) => {}
                }
            }
        }

        for &(usage_page, usage) in self.arrays.iter() {
            if !new_arrays.contains(&(usage_page, usage)) {
                // Release array items missing from new_arrays
                events.push(ReportEvent {
                    usage_page,
                    usage,
                    value: 0,
                    relative: false,
                });
            }
        }

        for &(usage_page, usage) in new_arrays.iter() {
            if !self.arrays.contains(&(usage_page, usage)) {
                // Press array items only in new_arrays
                events.push(ReportEvent {
                    usage_page,
                    usage,
                    value: 1,
                    relative: false,
                });
            }
        }

        //TODO: more efficient update of array items that does not involve allocation
        self.arrays = new_arrays;

        Ok(events)
    }
}
