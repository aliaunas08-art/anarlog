import { z } from "zod";

// The CSP has no `unsafe-eval` (ASPC A3.1). Without this, Zod probes
// `new Function` and the webview reports a CSP violation.
z.config({ jitless: true });
