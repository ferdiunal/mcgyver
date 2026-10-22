//! Apps' icons as egui textures, looked up on a thread so a frame never waits for the disk, the
//! icon theme or the shell. Asked for an icon it doesn't have yet, a cache starts the lookup and
//! answers `None`; the frame after the pixels arrive, it has a texture.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};

use crate::icons::{self, Icon};

/// Icons by app id, at one size.
pub struct Textures {
    px: u32,
    /// What the threads found, found or not.
    found: Arc<Mutex<HashMap<String, Option<Icon>>>>,
    /// What has been asked for already.
    asked: HashMap<String, Option<TextureHandle>>,
}

impl Default for Textures {
    /// Icons big enough for a wedge on a screen of twice the pixels.
    fn default() -> Textures {
        Textures::new(64)
    }
}

impl Textures {
    /// Icons about `px` pixels square.
    pub fn new(px: u32) -> Textures {
        Textures {
            px,
            found: Arc::default(),
            asked: HashMap::new(),
        }
    }

    /// The icon of the app `id` (as `icons::app` takes it), once it has been found.
    pub fn get(&mut self, ctx: &Context, id: &str) -> Option<TextureId> {
        if let Some(texture) = self.asked.get(id) {
            if let Some(texture) = texture {
                return Some(texture.id());
            }
            let icon = self
                .found
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(id)?;
            let texture = icon.map(|icon| {
                let image = ColorImage::from_rgba_unmultiplied(icon.size, &icon.rgba);
                ctx.load_texture(format!("icon {id}"), image, TextureOptions::LINEAR)
            });
            let found = texture.as_ref().map(TextureHandle::id);
            self.asked.insert(id.to_owned(), texture);
            return found;
        }
        self.asked.insert(id.to_owned(), None);
        let (found, id, px, ctx) = (self.found.clone(), id.to_owned(), self.px, ctx.clone());
        std::thread::spawn(move || {
            let icon = icons::app(&id, px);
            found
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id, icon);
            ctx.request_repaint();
        });
        None
    }
}
