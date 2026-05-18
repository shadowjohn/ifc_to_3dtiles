use std::collections::HashMap;

use crate::step::{StepIndex, extract_first_ref, extract_refs, numbers_in, split_arguments};

#[derive(Debug, Clone)]
pub struct StyleTable {
    item_colors: HashMap<u32, [f32; 4]>,
}

impl StyleTable {
    pub fn from_index(index: &StepIndex) -> Self {
        let mut colors = HashMap::<u32, [f32; 4]>::new();
        for entity in index.entities_by_type("IFCCOLOURRGB") {
            let nums = numbers_in(index.body(entity));
            if nums.len() >= 3 {
                colors.insert(
                    entity.id,
                    [nums[0] as f32, nums[1] as f32, nums[2] as f32, 1.0],
                );
            }
        }

        let mut shading_to_color = HashMap::new();
        for entity in index.entities_by_type("IFCSURFACESTYLESHADING") {
            if let Some(color_ref) = extract_first_ref(index.body(entity))
                && let Some(color) = colors.get(&color_ref)
            {
                shading_to_color.insert(entity.id, *color);
            }
        }

        let mut surface_to_color = HashMap::new();
        for entity in index.entities_by_type("IFCSURFACESTYLE") {
            for ref_id in extract_refs(index.body(entity)) {
                if let Some(color) = shading_to_color.get(&ref_id) {
                    surface_to_color.insert(entity.id, *color);
                    break;
                }
            }
        }

        let mut assignment_to_color = HashMap::new();
        for entity in index.entities_by_type("IFCPRESENTATIONSTYLEASSIGNMENT") {
            for ref_id in extract_refs(index.body(entity)) {
                if let Some(color) = surface_to_color.get(&ref_id) {
                    assignment_to_color.insert(entity.id, *color);
                    break;
                }
            }
        }

        let mut item_colors = HashMap::new();
        for entity in index.entities_by_type("IFCSTYLEDITEM") {
            let args = split_arguments(index.body(entity));
            let Some(item_id) = args.first().and_then(|arg| extract_first_ref(arg)) else {
                continue;
            };
            for ref_id in args.get(1).map(|arg| extract_refs(arg)).unwrap_or_default() {
                if let Some(color) = assignment_to_color.get(&ref_id) {
                    item_colors.insert(item_id, *color);
                    break;
                }
            }
        }

        Self { item_colors }
    }

    pub fn color_for_item(&self, item_id: u32) -> Option<[f32; 4]> {
        self.item_colors.get(&item_id).copied()
    }

    pub fn len(&self) -> usize {
        self.item_colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.item_colors.is_empty()
    }
}
