import { autostartAtom, currentMediaAtom, currentAppAtom } from "./state";
import { useAtom, useAtomValue } from "jotai";
import { useId } from "react";

export default function App() {
	return (
		<div>
			<div className="container">
				<Connection />
			</div>
			<div className="container">
				<CurrentMedia />
				<AutostartToggle />
			</div>
		</div>
	);
}

function CurrentMedia() {
	const media = useAtomValue(currentMediaAtom);
	if (!media) return;

	const artworkDataUrl = `data:${media.artwork_mime};base64,${media.artwork_bytes}`;

	return (
		<>
			{media.artwork_bytes && (
				<img
					src={artworkDataUrl}
					alt=""
					style={{
						width: "fit-content",
						height: "auto",
						maxWidth: "none",
						objectFit: "none",
					}}
				/>
			)}
			<h1 id="title">{media.title}</h1>
			<h2 id="artist">{media.artist}</h2>
		</>
	);
}

function AutostartToggle() {
	const id = useId();
	const [enabled, setEnabled] = useAtom(autostartAtom);

	return (
		<>
			<input
				type="checkbox"
				id={id}
				checked={enabled}
				onChange={() => setEnabled()}
			/>
			<label htmlFor={id}>Autostart</label>
		</>
	);
}

function Connection() {
	const id = useId();
	const [currentApp, setCurrentApp] = useAtom(currentAppAtom);

	const handleSubmit = (event) => {
		event.preventDefault();

		const data = new FormData(event.target);
		setCurrentApp(data.get("appId"));
	};

	return (
		<form onSubmit={handleSubmit}>
			<label htmlFor={id}>Application ID</label>
			<input type="text" id={id} name="appId" defaultValue={currentApp} />
			<button type="submit">Connect</button>
		</form>
	);
}
