//! Slicers: finding the ones installed, reading their filaments and opening a model in the
//! chosen one, and the status of the printers connected in it. Slicing and sending to the
//! printer stay in the slicer for now.

mod detect;
mod host_files;
mod profiles;
mod status;

pub use detect::{detect, detect_in, from_path, open, print_dir, Slicer};
pub use profiles::{filaments, printers, processes, project_settings, Filament};
pub use status::{connected, status, Printer, State, Status};
