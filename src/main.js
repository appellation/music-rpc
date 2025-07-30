import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

async function getMedia() {
	const media = await invoke("get_media");
	await setActivity(media);
}

async function setActivity(media) {
	if (media) {
		document.getElementById("artwork").style = "display: block";
		document.getElementById("artwork").src =
			`data:${media.artwork_mime};base64,${media.artwork_bytes}`;
	} else {
		document.getElementById("artwork").style = "display: none";
	}

	document.getElementById("title").innerText = media?.title ?? "";
	document.getElementById("artist").innerText = media?.artist ?? "";
	await invoke("set_activity", { media });
}

getMedia().catch(console.error);

listen("media_change", ({ payload }) => {
	setActivity(payload);
});

invoke("subscribe_media").catch(console.error);

const autostartCheckbox = document.getElementById("autostart");
autostartCheckbox.checked = await isEnabled();
autostartCheckbox.addEventListener("change", (event) => {
	if (event.target.checked) enable();
	else disable();
});
