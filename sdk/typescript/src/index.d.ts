import type { Condition, Operations, Target } from './ops.generated.js';
export type * from './ops.generated.js';

export interface LaunchOptions {
  /** Path to the `swiftbrowse` binary (default: $SWIFTBROWSE_BIN, then the binary installed for this platform, then PATH). */
  executable?: string;
  /** Test defaults (default true): no approval prompts for "Delete"/"Pay" clicks, `evaluate` allowed, uploads from the working directory. */
  testDefaults?: boolean;
  headed?: boolean;
  /** Launch a browser per process instead of attaching to the warm daemon. */
  noDaemon?: boolean;
  chromium?: string;
  evidenceDir?: string;
  /** Default action timeout in ms. */
  timeoutMs?: number;
  allowHosts?: string[];
  blockHosts?: string[];
  uploadDirs?: string[];
  allowFileUrls?: boolean;
  /** Throw a SwiftBrowseError when an operation fails (default true). */
  throwOnError?: boolean;
}

export interface SessionOptions {
  headless?: boolean;
  viewport?: [number, number];
  throwOnError?: boolean;
}

export interface Check {
  condition: Condition;
  passed: boolean;
  observed: unknown;
}

/** What every operation returns. */
export interface ActionResult {
  ok: boolean;
  op: string;
  action_id: string;
  session_id: string;
  target?: { element: Record<string, unknown>; strategy: string; candidates: number };
  page?: { url: string; title: string };
  verification?: { status: 'passed' | 'failed' | 'not_checked'; checks: Check[]; note?: string };
  timing: { started_at_ms: number; duration_ms: number; timeout_ms?: number };
  data?: any;
  error?: { kind: string; message: string; recoverable: boolean; details?: unknown };
  evidence: { kind: string; path: string; action_id: string }[];
  notes: string[];
}

export declare class SwiftBrowseError extends Error {
  kind?: string;
  result: ActionResult;
}

export declare function launch(options?: LaunchOptions): Promise<Browser>;

export declare class Browser {
  version: string;
  newSession(options?: SessionOptions): Promise<Session>;
  request(method: string, params?: Record<string, unknown>): Promise<any>;
  close(): Promise<void>;
}

export interface Session extends Operations {}
export declare class Session {
  id: string;
  run(op: string, args?: Record<string, unknown>): Promise<ActionResult>;
  /** Auto-waiting assertion over one condition or several. */
  expect(condition: Condition | Condition[], options?: { timeoutMs?: number; name?: string }): Promise<ActionResult>;
  /** The page as compact text. */
  text(): Promise<string>;
  close(): Promise<void>;
}

export type { Target };
