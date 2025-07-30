import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";
import { HTTPException } from "hono/http-exception";
import { DateTime, Duration } from "luxon";

const app = new Hono();

const MAX_DURATION = Duration.fromMillis(2 * 60 * 60 * 1000); // 2 hours

app.get("/:hash", async (ctx) => {
	const hash = ctx.req.param("hash");
	const { value, metadata } = await ctx.env.artwork.getWithMetadata(
		hash,
		"stream",
	);

	ctx.header("content-type", metadata.contentType);
	return ctx.body(value);
});

app.put(
	"/:hash",
	bodyLimit({
		maxSize: 500 * 1024, // 500kb
	}),
	async (ctx) => {
		const expiresAtStr = ctx.req.query("expires_at");
		if (!expiresAtStr)
			throw new HTTPException(400, {
				message: "expires_at query parameter must be present",
			});

		let expiresAt = DateTime.fromISO(expiresAtStr);
		if (!expiresAt.isValid) {
			throw new HTTPException(400, {
				message: "expires_at must be valid ISO8601",
			});
		}

		const now = DateTime.now();
		const maxExpiry = now.plus(MAX_DURATION);
		if (expiresAt.valueOf() >= maxExpiry.valueOf())
			throw new HTTPException(400, {
				message: `expires_at must be less than ${maxExpiry}`,
			});
		if (expiresAt.diff(now).as("minutes") < 1)
			expiresAt = now.plus(Duration.fromMillis(60 * 1000));

		const hash = ctx.req.param("hash");
		const contentType = ctx.req.header("Content-Type");
		await ctx.env.artwork.put(hash, ctx.req.raw.body, {
			expiration: expiresAt.toSeconds(),
			metadata: { contentType },
		});

		ctx.status(201);
		return ctx.body();
	},
);

export default app;
