use crate::{
    filters::{setdefaultlocation::adjust_axes, FontFilter},
    Font, LayerType,
};
use fontdrasil::coords::DesignLocation;

/// A filter that adds another font as a new master to the current font, optionally at a specified location
pub struct AddMaster {
    font: Font,
    location: Option<DesignLocation>,
}

impl AddMaster {
    /// Create a new AddMaster filter
    pub fn new(font: Font, location: Option<DesignLocation>) -> Self {
        AddMaster { font, location }
    }
}

impl FontFilter for AddMaster {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        if self.font.masters.len() != 1 {
            return Err(crate::BabelfontError::FilterError(
                "Added font must be a single-master font".into(),
            ));
        }
        let mut new_master = self.font.masters[0].clone();
        if let Some(loc) = &self.location {
            new_master.location = loc.clone();
            adjust_axes(&mut font.axes, loc)?;
        }
        new_master.name = self
            .font
            .source
            .clone()
            .and_then(|x| x.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Unnamed Master".into())
            .into();
        let old_incoming_master_id = new_master.id.clone();
        new_master.id = uuid::Uuid::new_v4().to_string();
        let new_master_id = new_master.id.clone();
        font.masters.push(new_master);
        // Add all layers from the new master to the font's glyphs
        for glyph in font.glyphs.iter_mut() {
            if let Some(layer) = self
                .font
                .glyphs
                .iter()
                .find(|g| g.name == glyph.name)
                .and_then(|g| {
                    g.layers.iter().find(|l| {
                        l.master == LayerType::DefaultForMaster(old_incoming_master_id.clone())
                            || l.master
                                == LayerType::AssociatedWithMaster(old_incoming_master_id.clone())
                    })
                })
            {
                let mut new_layer = layer.clone();
                new_layer.master = match new_layer.master {
                    LayerType::DefaultForMaster(_) => {
                        LayerType::DefaultForMaster(new_master_id.clone())
                    }
                    LayerType::AssociatedWithMaster(_) => {
                        LayerType::AssociatedWithMaster(new_master_id.clone())
                    }
                    _ => new_layer.master.clone(),
                };

                glyph.layers.push(new_layer);
            }
        }
        Ok(())
    }
    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let parts = s.splitn(2, '@').collect::<Vec<_>>();
        let font_path = parts[0].trim();
        let location = if parts.len() > 1 {
            Some(crate::filters::parse_location(parts[1].trim())?)
        } else {
            None
        };
        let font = crate::load(font_path)?;
        Ok(AddMaster::new(font, location))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("addmaster")
            .long("add-master")
            .help("Add another font as a new master to the current font")
            .value_name("FONT[@LOCATION]")
            .action(clap::ArgAction::Append)
    }
}
