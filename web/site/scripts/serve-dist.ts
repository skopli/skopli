import { serveDist } from "@skopli/ui/scripts/serve-dist.ts";

const port = Number(process.env.PORT ?? 4322);
await serveDist(new URL("../dist", import.meta.url).pathname, port, "404.html");
console.log(`serving dist on http://localhost:${port}/`);
