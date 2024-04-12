use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;

use crate::{
    binary_view::BinaryView,
    report_desc::{
        self, GlobalItemsState, LocalItemsState, MainCollectionFlags, MainItem, MainItemFlags,
        ReportFlatIter, ReportItem, ReportIter, ReportIterItem,
    },
};

fn div_round_up(num: usize, denom: usize) -> usize {
    if num % denom == 0 {
        num / denom
    } else {
        num / denom + 1
    }
}

#[derive(Debug)]
pub struct ReportEvent {
    pub usage_page: u32,
    pub usage: u32,
    pub value: i32,
    pub relative: bool,
}

#[derive(Debug)]
pub struct ReportInput {
    pub bit_length: usize,
    pub bit_offset: usize,
    pub global_state: GlobalItemsState,
    pub local_state: LocalItemsState,
    pub flags: MainItemFlags,
}

pub struct ReportHandler {
    pub inputs: Vec<ReportInput>,
    pub total_byte_length: usize,
    pub absolutes: HashMap<(u32, u32), i32>,
    pub arrays: HashSet<(u32, u32)>,
}

impl ReportHandler {
    pub fn new(descriptor: &[u8]) -> Result<Self, Box<dyn Error>> {
        let report_desc = ReportIter::new(ReportFlatIter::new(descriptor)).collect::<Vec<_>>();

        for item in &report_desc {
            log::debug!("{:X?}", item);
        }

        let (mut global_state, mut local_state, mut stack) = (
            GlobalItemsState::default(),
            LocalItemsState::default(),
            Vec::new(),
        );

        //TODO: parse other collections
        let (_, application_collection, application_global_state, application_local_state) =
            report_desc
                .iter()
                .filter_map(|item: &ReportIterItem| {
                    log::trace!("1: {:?}", item);
                    match item {
                        &ReportIterItem::Item(ref item) => {
                            report_desc::update_global_state(&mut global_state, &mut stack, item)
                                .unwrap();
                            report_desc::update_local_state(&mut local_state, item);
                            None
                        }
                        &ReportIterItem::Collection(n, ref collection) => {
                            let lc_state =
                                std::mem::replace(&mut local_state, LocalItemsState::default());
                            Some((n, collection, global_state, lc_state))
                        }
                    }
                })
                .find(|&(n, _, _, _)| n == MainCollectionFlags::Application as u8)
                .expect("Failed to find application collection");

        let mut collections = VecDeque::new();
        collections.push_back(application_collection);
        let mut items = Vec::new();
        while let Some(collection) = collections.pop_front() {
            for item in collection {
                log::trace!("2: {:?}", item);
                match item {
                    ReportIterItem::Item(item) => match item {
                        ReportItem::Global(_) => {
                            report_desc::update_global_state(&mut global_state, &mut stack, item)
                                .unwrap();
                        }
                        ReportItem::Main(m) => {
                            let lc_state =
                                std::mem::replace(&mut local_state, LocalItemsState::default());
                            items.push((global_state, lc_state, m));
                        }
                        ReportItem::Local(_) => {
                            report_desc::update_local_state(&mut local_state, item);
                        }
                    },
                    //TODO: does local state need to be different for inner collections?
                    ReportIterItem::Collection(_, collection) => {
                        collections.push_back(collection);
                    }
                }
            }
        }

        let mut total_bit_length = 0;
        let inputs = items
            .iter()
            .filter_map(|(global_state, local_state, item)| {
                log::trace!("3: {:?}", item);

                if let &MainItem::Input(flags) = item {
                    let report_size = match global_state.report_size {
                        Some(s) => s,
                        None => return None,
                    };
                    let report_count = match global_state.report_count {
                        Some(c) => c,
                        None => return None,
                    };

                    let bit_length = (report_size * report_count) as usize;
                    let bit_offset = total_bit_length;
                    total_bit_length += bit_length;

                    Some(ReportInput {
                        bit_length,
                        bit_offset,
                        global_state: *global_state,
                        local_state: local_state.clone(),
                        flags: MainItemFlags::from_bits_truncate(*flags),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let total_byte_length = div_round_up(total_bit_length, 8);

        Ok(Self {
            inputs,
            total_byte_length,
            absolutes: HashMap::new(),
            arrays: HashSet::new(),
        })
    }

    pub fn handle(&mut self, report: &[u8]) -> Result<Vec<ReportEvent>, Box<dyn Error>> {
        let mut events = Vec::new();
        let mut new_arrays = HashSet::new();
        for ReportInput {
            bit_length,
            bit_offset,
            global_state,
            local_state,
            flags,
        } in &self.inputs
        {
            //TODO: report ID
            let report_size = global_state.report_size.unwrap();
            let report_count = global_state.report_count.unwrap();
            let variable = flags.contains(MainItemFlags::VARIABLE);
            let relative = flags.contains(MainItemFlags::RELATIVE);

            log::debug!(
                "  size {} count {} offset {} length {} usage page {:X?} usages {:X?} usage_min {:X?}, usage_max {:X?} flags {:?}",
                report_size, report_count, bit_offset, bit_length, global_state.usage_page, local_state.usages, local_state.usage_min, local_state.usage_max, flags
            );

            let binary_view = BinaryView::new(report, *bit_offset, *bit_length);

            for report_i in 0..report_count {
                let report_bit_index = (report_i * report_size) as usize;
                log::trace!("    {report_i}@{report_bit_index}");

                let mut value = 0i32;
                for bit_index in 0..report_size {
                    match binary_view.get(report_bit_index + bit_index as usize) {
                        Some(bit) => {
                            if bit {
                                value |= 1 << bit_index;
                            }
                        }
                        None => {
                            //TODO: error?
                        }
                    }
                }

                let Some(usage_page) = global_state.usage_page else {
                    //TODO: error?
                    log::trace!("      usage page not found");
                    continue;
                };
                let usage = if variable {
                    match local_state.usage_min {
                        Some(usage_min) => usage_min + report_i,
                        None => match local_state.usages.get(report_i as usize).copied() {
                            Some(usage) => usage,
                            None => {
                                //TODO: error?
                                log::trace!("      usage not found");
                                continue;
                            }
                        },
                    }
                } else {
                    let usage = value as u32;
                    value = 1;
                    usage
                };
                log::trace!("      usage 0x{usage_page:X}:0x{usage:X}");

                let logical_min = global_state.logical_min.unwrap_or(0);
                let logical_max = global_state.logical_max.unwrap_or((1 << report_size) - 1);
                log::trace!("      logical 0x{logical_min:X}:0x{logical_max:X}");

                if logical_min > logical_max {
                    // Sign extend if logical_min is less than logical max, meaning the value is
                    // signed using two's complement
                    for bit_index in report_size..32 {
                        if value & (1 << (bit_index - 1)) != 0 {
                            value |= 1 << bit_index;
                        } else {
                            break;
                        }
                    }
                }

                log::trace!("      value 0x{value:0X}");

                if !variable {
                    new_arrays.insert((usage_page, usage));
                    // Array items processed later
                    continue;
                }

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
        }

        for &(usage_page, usage) in self.arrays.iter() {
            if !new_arrays.contains(&(usage_page, usage)) {
                // Release array items missing from new_arrays
                events.push(ReportEvent {
                    usage_page,
                    usage,
                    value: 0,
                    //TODO: use relative from above? What is a relative array?
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
                    //TODO: use relative from above? What is a relative array?
                    relative: false,
                });
            }
        }

        //TODO: more efficient update of array items that does not involve allocation
        self.arrays = new_arrays;

        Ok(events)
    }
}
