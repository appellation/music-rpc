import { defineConfig } from "vite";

export default defineConfig({
	// prevent vite from obscuring rust errors
	clearScreen: false,
	// Tauri expects a fixed port, fail if that port is not available
	server: {
		strictPort: true,
	},
	// to access the Tauri environment variables set by the CLI with information about the current target
	envPrefix: [
		"VITE_",
		"TAURI_PLATFORM",
		"TAURI_ARCH",
		"TAURI_FAMILY",
		"TAURI_PLATFORM_VERSION",
		"TAURI_PLATFORM_TYPE",
		"TAURI_DEBUG",
	],
	build: {
		// Tauri uses Chromium on Windows and WebKit on macOS and Linux
		// target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari13",
		// TODO: remove this once Tidal stops using top level await?
		target: "esnext",
		// don't minify for debug builds
		minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
		// produce sourcemaps for debug builds
		sourcemap: !!process.env.TAURI_DEBUG,
	},
	esbuild: {
		// target: ["es2022", "chrome115"],
		supported: {
			"top-level-await": true,
		},
	},
	optimizeDeps: {
		esbuildOptions: {
			supported: {
				"top-level-await": true,
			},
		},
	},
});
