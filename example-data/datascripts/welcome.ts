import { run } from "../../../datascripts/lib/index.ts";

await run("example-data", (data) => {
  data.spell(133).clone(6_000_300).set("name", "A Warm Welcome");
});
