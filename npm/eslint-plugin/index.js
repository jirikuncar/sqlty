import metadata from "./package.json" with { type: "json" };

const plugin = {
  meta: { name: metadata.name, version: metadata.version },
  rules: {},
};

export default plugin;
