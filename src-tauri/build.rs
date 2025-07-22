use dotenv_build::Config;

fn main() {
	let _ = dotenv_build::output(Config::default());
	tauri_build::build()
}
