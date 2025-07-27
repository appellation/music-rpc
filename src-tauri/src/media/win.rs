use std::thread;

use blake3::hash;
use futures::TryStream;
use jiff::{SignedDuration, Timestamp};
use tauri::{
	async_runtime::{block_on, spawn_blocking},
	AppHandle,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use windows::{
	Foundation::{DateTime, TimeSpan, TypedEventHandler},
	Media::Control::{
		GlobalSystemMediaTransportControlsSession,
		GlobalSystemMediaTransportControlsSessionManager,
		GlobalSystemMediaTransportControlsSessionMediaProperties,
		GlobalSystemMediaTransportControlsSessionTimelineProperties,
	},
	Storage::Streams::{DataReader, IRandomAccessStreamWithContentType},
};
use windows_core::Ref;

use crate::{error::AppResult, media::Media};

/// The universal time epoch is midnight on January 1, 1601 in the Gregorian calendar
fn universal_epoch() -> Timestamp {
	Timestamp::from_duration(
		// This is the difference between the Unix epoch and the universal time epoch
		SignedDuration::from_secs(-11_644_473_600),
	)
	.unwrap()
}

fn from_date_time(date_time: DateTime) -> Timestamp {
	// universal time is expressed in 100-nanosecond units
	// unfortunately we can't convert it into true nanoseconds without overflowing an i64, so we convert it into microseconds
	universal_epoch() + SignedDuration::from_micros(date_time.UniversalTime / 10)
}

fn from_time_span(time_span: TimeSpan) -> SignedDuration {
	// time spans are expressed in 100-nanosecond units
	SignedDuration::from_nanos(time_span.Duration * 100)
}

fn read_stream_to_vec(
	stream: &IRandomAccessStreamWithContentType,
) -> windows_core::Result<Vec<u8>> {
	let size = stream.Size()? as u32;
	let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
	reader.LoadAsync(size)?.get()?;

	let mut buffer = vec![0u8; size as usize];
	reader.ReadBytes(&mut buffer)?;

	Ok(buffer)
}

pub async fn subscribe(
	_app: AppHandle,
) -> AppResult<impl TryStream<Ok = Option<Media>, Error = anyhow::Error>> {
	let (tx, rx) = mpsc::channel(32);

	let session_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.await?;

	let current_session = session_manager.GetCurrentSession()?;
	let tx2 = tx.clone();
	thread::spawn(move || subscribe_session(current_session, tx2));

	let session_changed_handler = TypedEventHandler::new(
		move |sender: Ref<GlobalSystemMediaTransportControlsSessionManager>, _| {
			if let Some(manager) = &*sender {
				let session = manager.GetCurrentSession()?;
				let tx = tx.clone();
				thread::spawn(move || subscribe_session(session, tx));
			}
			Ok(())
		},
	);
	session_manager.CurrentSessionChanged(&session_changed_handler)?;

	Ok(ReceiverStream::new(rx))
}

fn subscribe_session(
	session: GlobalSystemMediaTransportControlsSession,
	tx: mpsc::Sender<anyhow::Result<Option<Media>>>,
) -> windows_core::Result<()> {
	let tx2 = tx.clone();
	let session_update =
		move |session: Ref<GlobalSystemMediaTransportControlsSession>| -> windows_core::Result<()> {
			match &*session {
				Some(session) => {
					let properties = session.TryGetMediaPropertiesAsync()?.get()?;
					let timeline = session.GetTimelineProperties()?;

					let _ = tx2.blocking_send(Ok(Some(Media::from_windows(timeline, properties)?)));
				}
				None => {
					let _ = tx2.blocking_send(Ok(None));
				}
			}
			Ok(())
		};
	let session_update2 = session_update.clone();

	let timeline_properties_changed_handler = TypedEventHandler::new(
		move |session: Ref<GlobalSystemMediaTransportControlsSession>, _| session_update(session),
	);
	let timeline_token = session.TimelinePropertiesChanged(&timeline_properties_changed_handler)?;

	let media_properties_changed_handler =
		TypedEventHandler::new(move |session, _| session_update2(session));
	let properties_token = session.MediaPropertiesChanged(&media_properties_changed_handler)?;

	block_on(tx.closed());

	let _ = session.RemoveTimelinePropertiesChanged(timeline_token);
	let _ = session.RemoveMediaPropertiesChanged(properties_token);

	Ok(())
}

pub async fn get(_app: AppHandle) -> AppResult<Option<Media>> {
	let session = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?
		.await?
		.GetCurrentSession()?;

	let properties = session.TryGetMediaPropertiesAsync()?.await?;
	let timeline = session.GetTimelineProperties()?;

	Ok(spawn_blocking(|| Media::from_windows(timeline, properties))
		.await?
		.map(Some)?)
}

impl Media {
	fn from_windows(
		timeline: GlobalSystemMediaTransportControlsSessionTimelineProperties,
		properties: GlobalSystemMediaTransportControlsSessionMediaProperties,
	) -> windows_core::Result<Self> {
		let last_updated = from_date_time(timeline.LastUpdatedTime()?);
		let elapsed = from_time_span(timeline.Position()?);
		let start_duration = from_time_span(timeline.StartTime()?);
		let start = last_updated - elapsed + start_duration;
		let end = start + from_time_span(timeline.EndTime()?) - start_duration;

		dbg!(&last_updated, &elapsed, &start_duration, &start, &end);

		let artwork_stream = properties.Thumbnail()?.OpenReadAsync()?.get()?;
		let artwork_mime = artwork_stream.ContentType()?.to_string_lossy();
		let artwork_bytes = read_stream_to_vec(&artwork_stream)?;
		let artwork_hash = hash(&artwork_bytes).to_hex().to_string();

		Ok(Media {
			title: properties.Title()?.to_string_lossy(),
			artist: properties.Artist()?.to_string_lossy(),
			start,
			end,
			artwork_mime,
			artwork_bytes,
			artwork_hash,
		})
	}
}
