import { NextResponse } from "next/server";
import { paypalAccessToken, paypalBase, paypalConfigured } from "@/lib/paypal";
import { validateBodySize } from "@/lib/store";

export const runtime = "nodejs";

/**
 * Create a PayPal order (intent: CAPTURE) for a USD tournament sponsorship.
 * Body: { amountUsd: number, tournamentName: string }
 * The amount is fixed server-side so the client can never underpay.
 */
export async function POST(request: Request) {
  if (validateBodySize(request)) return validateBodySize(request)!;
  if (!paypalConfigured()) {
    return NextResponse.json(
      { error: "PayPal not configured. Set PAYPAL_CLIENT_ID / PAYPAL_SECRET." },
      { status: 503 }
    );
  }
  const body = (await request.json().catch(() => ({}))) as {
    amountUsd?: number;
    tournamentName?: string;
  };
  const amount = Number(body.amountUsd);
  if (!Number.isFinite(amount) || amount < 1 || amount > 10_000) {
    return NextResponse.json({ error: "amountUsd must be between 1 and 10000" }, { status: 400 });
  }

  try {
    const token = await paypalAccessToken();
    const res = await fetch(`${paypalBase()}/v2/checkout/orders`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        intent: "CAPTURE",
        purchase_units: [
          {
            amount: { currency_code: "USD", value: amount.toFixed(2) },
            description: `AMP Tournament — ${body.tournamentName || "Community Cup"}`,
            custom_id: `amp-cup-${Date.now()}`,
          },
        ],
      }),
    });
    if (!res.ok) {
      return NextResponse.json({ error: `PayPal create failed (${res.status})` }, { status: 502 });
    }
    const order = (await res.json()) as { id: string };
    return NextResponse.json({ id: order.id });
  } catch (e) {
    return NextResponse.json({ error: (e as Error).message }, { status: 500 });
  }
}
