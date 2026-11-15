import { describe, expect, it } from 'vitest';

import { ApiFailure } from '../api';
import { errorArt } from './ErrorPage';

describe('errorArt', () => {
  it('maps 403 to the angry cat, 5xx to the repair cat, and the rest to the surprised cat', () => {
    expect(errorArt(new ApiFailure(403, 'not_admin'))).toBe('angry');
    expect(errorArt(new ApiFailure(403, 'forbidden'))).toBe('angry');
    expect(errorArt(new ApiFailure(500, 'internal'))).toBe('repair');
    expect(errorArt(new ApiFailure(503, 'unavailable'))).toBe('repair');
    expect(errorArt(new ApiFailure(0, 'no_answer'))).toBe('surprised');
    expect(errorArt(new ApiFailure(404, 'not_found'))).toBe('surprised');
    expect(errorArt(new ApiFailure(418, 'teapot'))).toBe('surprised');
    expect(errorArt(new Error('boom'))).toBe('surprised');
  });
});
