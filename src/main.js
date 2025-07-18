import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

async function getMedia() {
  const media = await invoke("get_media");
  await setActivity(media);
}

async function setActivity(media) {
  document.getElementById("title").innerText = media.title;
  document.getElementById("artist").innerText = media.artist;
  await invoke("set_activity", { properties: media });
}

getMedia().catch(console.error);

listen("media_change", ({ payload }) => {
  setActivity(payload);
});

invoke("subscribe_media").catch(console.error);
