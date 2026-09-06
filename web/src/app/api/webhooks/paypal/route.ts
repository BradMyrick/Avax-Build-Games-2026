import { NextResponse } from "next/server";
import { paypalAccessToken, paypalBase, paypalConfigured } from "@/lib/paypal";
import { getStore, validateBodySize } from "@/lib/store";

export const runtime = "nodejs";

/**
 * POST /api/webhooks/paypal — PayPal webhook receiver.
 * Verifies the signature via PayPal's verify-webhook-signature API.
 * On PAYMENT.CAPTURE.COMPLETED, acts as a reliability fallback: if the capture
 * route never processed this order (network failure), the webhook enqueues a
 * fund job. Idempotent via the funding_intents table.
 *
 * Always returns 200 so PayPal doesn't retry (retries would double-process).
 */
export async function POST(request: Request) {
  if (validateBodySize(request)) return validateBodySize(request)!;
  // Always return 200 to stop retries, even on misconfiguration.
  if (!paypalConfigured()) return new NextResponse("ok", { status: 200 });

  const webhookId = process.env.PAYPAL_WEBHOOK_ID;
  if (!webhookId) return new NextResponse("ok", { status: 200 });

  const rawBody = await request.text();
  let event: Record<string, unknown>;
  try {
    event = JSON.parse(rawBody);
  } catch {
    return new NextResponse("ok", { status: 200 });
  }

  // Verify the webhook signature.
  try {
    const token = await paypalAccessToken();
    const get = (h: string) => request.headers.get(h) ?? "";
    const verifyRes = await fetch(`${paypalBase()}/v1/notifications/verify-webhook-signature`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({
        auth_algo: get("paypal-auth-algo"),
        cert_url: get("paypal-cert-url"),
        transmission_id: get("paypal-transmission-id"),
        transmission_sig: get("paypal-transmission-sig"),
        transmission_time: get("paypal-transmission-time"),
        webhook_id: webhookId,
        webhook_event: event,
      }),
    });
    const verifyJson = (await verifyRes.json()) as { verification_status?: string };
    if (verifyJson.verification_status !== "SUCCESS") {
      return new NextResponse("ok", { status: 200 });
    }
  } catch {
    return new NextResponse("ok", { status: 200 });
  }

  // Fulfill on PAYMENT.CAPTURE.COMPLETED (reliability fallback to the capture route).
  if (event.event_type === "PAYMENT.CAPTURE.COMPLETED") {
    const resource = event.resource as Record<string, unknown> | undefined;
    const orderId =
      (resource?.supplementary_data as { related_ids?: { order_id?: string } })?.related_ids?.order_id
      ?? (resource?.id as string)
      ?? "";
    const amount = parseFloat(
      (resource?.amount as { value?: string })?.value ?? "0"
    );
    if (orderId && amount > 0) {
      const store = getStore();
      const intent = await store.claimFundingIntent(orderId, amount);
      if (intent.first) {
        // Capture route never processed this order — enqueue a fallback.
        // Note: the webhook lacks the original tournament config (payoutBps,
        // players). This creates a generic instant tournament; the admin
        // reconciles using the PayPal dashboard for the specific details.
        await store.enqueueJob("fund", {
          payoutBps: [10000],
          winnerWallets: [],
          fundedAvax: amount,
          mode: "instant",
          finalizeDays: 7,
          paypalOrderId: orderId,
        });
      }
    }
  }

  return new NextResponse("ok", { status: 200 });
}
