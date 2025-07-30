import { Store } from "@tauri-apps/plugin-store";
import { atom } from "jotai";
import { observe } from "jotai-effect";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

export const storeAtom = atom(() => Store.load("store.json"));

export const currentAppAtom = atom(
	async (get) => {
		const store = await get(storeAtom);
		return store.get("appId");
	},
	async (get, _set, value) => {
		const store = await get(storeAtom);
		await store.set("appId", value);
	},
);

export const isConnectedAtom = atom(false);

observe(async (get, set) => {
	const appId = await get(currentAppAtom);
	try {
		await invoke("connect", { clientId: appId });
		set(isConnectedAtom, true);
	} catch {
		set(isConnectedAtom, false);
	}
});

export const currentMediaAtom = atom();
currentMediaAtom.onMount = async (setAtom) => {
	const value = await invoke("get_media");
	setAtom(value);

	const unlisten = await listen("media_change", ({ payload }) => {
		setAtom(payload);
	});

	return unlisten;
};

observe(async (get) => {
	const media = await get(currentMediaAtom);
	const isConnected = get(isConnectedAtom);
	if (isConnected) invoke("set_activity", { media });
});

export const autostartAtom = atom(
	() => {
		return isEnabled();
	},
	async (_get, _set, value) => {
		if (value) await enable();
		else await disable();
	},
);
