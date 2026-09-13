/// Shared HTTP-response helpers for the frontend unit tests.
///
/// Almost every suite stubs `fetch` with a JSON body and asserts the parsed
/// result, so the `ok` helper lives here once instead of being re-declared in
/// every `*.test.ts(x)` file (SonarCloud flagged those copies as duplicated
/// code). This file is excluded from coverage: it holds no application logic.
export function ok(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), { status })
}
