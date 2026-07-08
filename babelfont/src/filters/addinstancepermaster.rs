use std::collections::HashSet;

use crate::{filters::FontFilter, BabelfontError, FormatSpecific, Instance, Names};

/// A filter that adds an instance for each master in the font
#[derive(Default)]
pub struct AddInstancePerMaster;

impl FontFilter for AddInstancePerMaster {
    fn apply(&self, font: &mut crate::Font) -> Result<(), BabelfontError> {
        let mut existing_instance_locations = HashSet::new();
        for instance in &font.instances {
            existing_instance_locations.insert(instance.location.clone());
        }

        for master in &font.masters {
            let instance_location = master.location.clone();
            if !existing_instance_locations.contains(&instance_location) {
                log::info!(
                    "Adding instance for master {} at location {:?}",
                    master.name.get_default().unwrap_or(&master.id),
                    instance_location
                );
                font.instances.push(Instance {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: master.name.clone(),
                    location: master.location.clone(),
                    custom_names: Names::default(),
                    variable: false,
                    linked_style: None,
                    format_specific: FormatSpecific::default(),
                })
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(AddInstancePerMaster)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("addinstancepermaster")
            .long("add-instance-per-master")
            .help("Add an instance for each master in the font")
            .action(clap::ArgAction::SetTrue)
    }
}
