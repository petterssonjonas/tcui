use color_eyre::Result;

pub struct EventHandler;

impl EventHandler {
    pub fn new(_tx: tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>) -> Result<Self> {
        Ok(Self)
    }
}
