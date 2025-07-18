use windows::{
	Foundation::TypedEventHandler,
	Media::Control::{
		GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
	},
};

fn universal_epoch() -> SystemTime {
	UNIX_EPOCH - Duration::from_secs(11_644_473_600)
}

pub async fn subscribe_media(app: AppHandle) -> AppResult<()> {
	thread::spawn(move || {
		let session_manager =
			GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;

		let current_session = session_manager.GetCurrentSession()?;
		subscribe_session(app.clone(), current_session)?;

		// let session_changed_handler = TypedEventHandler::new(
		// 	move |sender: windows_core::Ref<GlobalSystemMediaTransportControlsSessionManager>, _| {
		// 		if let Some(manager) = &*sender {
		// 			let session = manager.GetCurrentSession().unwrap();
		// 			// subscribe_session(app.clone(), session).unwrap();
		// 		}

		// 		println!("session changed");
		// 		emit_media_change(app.clone());
		// 		Ok(())
		// 	},
		// );
		// session_manager.CurrentSessionChanged(&session_changed_handler)?;

		Ok::<_, AppError>(())
	});

	Ok(())
}

fn subscribe_session(
	app: AppHandle,
	session: GlobalSystemMediaTransportControlsSession,
) -> AppResult<()> {
	let app2 = app.clone();
	let timeline_properties_changed_handler = TypedEventHandler::new(move |_, _| {
		emit_media_change(app2.clone());
		Ok(())
	});
	let _timeline_token =
		session.TimelinePropertiesChanged(&timeline_properties_changed_handler)?;

	let media_properties_changed_handler = TypedEventHandler::new(move |_, _| {
		emit_media_change(app.clone());
		Ok(())
	});
	let _media_properties_token =
		session.MediaPropertiesChanged(&media_properties_changed_handler)?;

	thread::park();
	Ok(())
}

pub async fn get_media() -> AppResult<Properties> {
	let session = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?
		.await?
		.GetCurrentSession()?;

	let properties = session.TryGetMediaPropertiesAsync()?.await?;
	let timeline = session.GetTimelineProperties()?;

	let last_updated = universal_epoch()
		+ Duration::from_nanos(timeline.LastUpdatedTime()?.UniversalTime as u64 * 100);
	let start =
		last_updated + Duration::from(timeline.StartTime()?) - Duration::from(timeline.Position()?);
	let end = start + Duration::from(timeline.EndTime()?);

	Ok(Properties {
		title: properties.Title()?.to_string_lossy(),
		artist: properties.Artist()?.to_string_lossy(),
		start: start.duration_since(UNIX_EPOCH)?.as_millis(),
		end: end.duration_since(UNIX_EPOCH)?.as_millis(),
	})
}
