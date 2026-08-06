import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

describe('drawing i18n', () => {
  it('defines drawing parameter and image protocol labels in every locale', () => {
    const localesDir = resolve(process.cwd(), 'src/i18n/locales');
    for (const fileName of readdirSync(localesDir).filter((name) => name.endsWith('.json'))) {
      const locale = JSON.parse(readFileSync(resolve(localesDir, fileName), 'utf8'));
      expect(locale.drawing, fileName).toMatchObject({
        noConfiguredImageProvider: expect.any(String),
        openProviderSettings: expect.any(String),
        advancedSettings: expect.any(String),
        generationApiPath: expect.any(String),
        editApiPath: expect.any(String),
        referenceImageParamName: expect.any(String),
        referenceImageParamNameHint: expect.any(String),
        operationUnavailable: expect.any(String),
        personGeneration: expect.any(String),
        resolution: expect.any(String),
        seed: expect.any(String),
        inferenceSteps: expect.any(String),
        guidanceScale: expect.any(String),
        aspectRatio: expect.any(String),
        referenceImageFormat: expect.any(String),
        option: {
          auto: expect.any(String),
          quality: {
            standard: expect.any(String),
            hd: expect.any(String),
            low: expect.any(String),
            medium: expect.any(String),
            high: expect.any(String),
          },
          background: {
            opaque: expect.any(String),
            transparent: expect.any(String),
          },
          referenceImageFormat: {
            object: expect.any(String),
            string: expect.any(String),
          },
          personGeneration: {
            dontAllow: expect.any(String),
            allowAdult: expect.any(String),
            allowAll: expect.any(String),
          },
        },
        warning: {
          unknown_image_profile: expect.any(String),
          using_fallback_profile: expect.any(String),
          compatibilityTitle: expect.any(String),
          legacy_model: expect.any(String),
          retired_model: expect.any(String),
          deprecated_model: expect.any(String),
          deadline: expect.any(String),
          replacement: expect.any(String),
          separator: expect.any(String),
        },
      });
      expect(locale.imageProtocol, fileName).toMatchObject({
        title: expect.any(String),
        grokAutoDetectDescription: expect.any(String),
        autoDetectDescription: expect.any(String),
        adapterProfile: expect.any(String),
        paramProfile: expect.any(String),
        paramProfileHint: expect.any(String),
        paramProfileAuto: expect.any(String),
        autoDetect: expect.any(String),
        capabilityOverrides: expect.any(String),
        operation: {
          generate: expect.any(String),
          edit: expect.any(String),
          maskEdit: expect.any(String),
        },
        generationEndpoint: expect.any(String),
        generationEndpointPlaceholder: expect.any(String),
        editEndpoint: expect.any(String),
        pollEndpoint: expect.any(String),
        cancelEndpoint: expect.any(String),
        authMode: expect.any(String),
        auth: {
          bearer: expect.any(String),
          apiKeyHeader: expect.any(String),
          query: expect.any(String),
          none: expect.any(String),
        },
        authHeaderName: expect.any(String),
        pollIntervalSeconds: expect.any(String),
        timeoutSeconds: expect.any(String),
        extraBody: expect.any(String),
        fieldResponseMapping: expect.any(String),
        jsonObjectRequired: expect.any(String),
        invalidJson: expect.any(String),
      });
    }
  });
});
