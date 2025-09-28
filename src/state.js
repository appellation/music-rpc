import { Store } from "@tauri-apps/plugin-store";
import { atom } from "jotai";
import { observe } from "jotai-effect";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { atomWithStorage, loadable } from "jotai/utils";

export const storeAtom = atom(() => Store.load("store.json"));

const storeStorage = new (class StoreStorage {
	#store;

	constructor() {
		this.#store = Store.load("store.json");
	}

	async getItem(key, initialValue) {
		const store = await this.#store;
		const value = await store.get(key);
		return value ?? initialValue;
	}

	async setItem(key, value) {
		const store = await this.#store;
		await store.set(key, value);
	}

	async removeItem(key) {
		const store = await this.#store;
		await store.delete(key);
	}

	async subscribe(key, callback, _initialValue) {
		const store = await this.#store;
		return store.onKeyChange(key, callback);
	}
})();

export const currentAppAtom = atomWithStorage("appId", null, storeStorage);
export const isConnectedAtom = atom(false);

const currentAppLoadableAtom = loadable(currentAppAtom);
observe((get, set) => {
	const { state, data } = get(currentAppLoadableAtom);
	if (state !== "hasData") return;

	const clientId = data === "" ? null : data;
	(async () => {
		try {
			const isConnected = await invoke("connect", { clientId });
			set(isConnectedAtom, isConnected);
		} catch {
			set(isConnectedAtom, false);
		}
	})();
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

observe((get) => {
	const media = get(currentMediaAtom);
	if (!media) return;

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
