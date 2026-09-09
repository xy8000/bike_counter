/// Pulls the @testing-library/jest-dom matcher types into the `tsc` program.
///
/// The Vitest setup (frontend/vitest.setup.tsx) imports
/// `@testing-library/jest-dom/vitest` at runtime, but that file lives outside
/// the `src` directory that tsconfig.json type-checks, so the ambient matcher
/// augmentations (`toBeInTheDocument`, `toHaveClass`, ...) are not visible to
/// `tsc` — every `npm run build`/`tsc` over the colocated `*.test.tsx` files
/// would fail with "Property does not exist on type Assertion<...>".
///
/// Importing the same module here (a type-only re-import inside `src`, which
/// tsconfig includes) registers the augmentations for the whole program.
import '@testing-library/jest-dom/vitest'
