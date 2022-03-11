pub mod tabs;

use warp::Constellation;
use warp_pocket_dimension::PocketDimension;



pub struct TerminalApplication {
	pub title: Option<String>,
	pub modules: Vec<Module>,
	pub cache: Option<Box<dyn PocketDimension>>,
	pub filesystem: Option<Box<dyn Constellation>>,
}

pub struct Module {

}

pub struct Extension {

}


