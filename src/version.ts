export type TinyTopDaemonMetadata = {
  os: string;
  arch: string;
  install: {
    executable: string;
    workingDirectory: string;
  };
  bind?: {
    host: string;
    port: number;
  };
  storage: {
    sqlitePath: string;
  };
};

export type TinyTopVersionMetadata = {
  status: "ok";
  app: "tinytop";
  version: string;
  runtime: "legacy-bun";
  component: "dashboard" | "collector";
  dashboard: "legacy" | "none";
  daemon?: TinyTopDaemonMetadata;
  collector?: unknown;
};

let productVersionPromise: Promise<string> | null = null;

export async function productVersion(): Promise<string> {
  productVersionPromise ??= Bun.file(new URL("../VERSION", import.meta.url).pathname).text();
  return (await productVersionPromise).trim();
}

export async function versionMetadata(
  options: Omit<TinyTopVersionMetadata, "status" | "app" | "version">,
): Promise<TinyTopVersionMetadata> {
  return {
    status: "ok",
    app: "tinytop",
    version: await productVersion(),
    ...options,
  };
}
