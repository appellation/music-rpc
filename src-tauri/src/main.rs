#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
	music_rpc_lib::run().unwrap();
}
