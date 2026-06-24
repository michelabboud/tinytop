declare module "node:os" {
  export type CpuInfo = unknown;

  const os: {
    hostname(): string;
    platform(): string;
    arch(): string;
    release(): string;
    cpus(): CpuInfo[];
  };

  export default os;
}

declare const Bun: {
  file(path: string): BodyInit & { text(): Promise<string> };
  sleep(milliseconds: number): Promise<void>;
  spawn(
    command: string[],
    options: {
      cwd?: string;
      stdin?: "pipe" | "inherit";
      stdout: "pipe" | "inherit";
      stderr: "pipe" | "inherit";
      env?: Record<string, string | undefined>;
    },
  ): {
    stdout: ReadableStream<Uint8Array>;
    exited: Promise<number>;
    kill(): void;
  };
  serve(options: {
    hostname?: string;
    port?: number;
    fetch(request: Request): Response | Promise<Response>;
  }): { url: string; stop(force?: boolean): void };
};

declare const process: {
  env: Record<string, string | undefined>;
  argv: string[];
  cwd(): string;
  exit(code?: number): never;
};
