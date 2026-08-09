import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const requiredKeys = [
  'interactionPermissionTitle',
  'interactionQuestionTitle',
  'interactionPlanReviewTitle',
  'interactionPlanApprove',
  'interactionPlanContinue',
  'interactionPlanAbandon',
  'interactionPlanExecute',
  'interactionPlanRequestChanges',
  'interactionPlanCancel',
  'interactionPlanFeedbackPlaceholder',
  'interactionPlanSubmitFeedback',
  'interactionPlanFeedbackRequired',
  'interactionPlanEmpty',
  'interactionPlanFullscreen',
  'interactionPlanExitFullscreen',
  'interactionPlanOutcomeApproved',
  'interactionPlanOutcomeChanges',
  'interactionPlanOutcomeAbandoned',
  'interactionPlanOutcomeExpired',
  'interactionPlanOutcomePending',
  'interactionPlanAddToContext',
  'interactionPlanAddedToContext',
  'interactionPlanFeedbackLabel',
  'interactionPrevItem',
  'interactionNextItem',
  'interactionPrevQuestion',
  'interactionNextQuestion',
  'interactionContinue',
  'interactionAllowAlways',
  'interactionAllowOnce',
  'interactionDeny',
  'interactionRequestDetails',
  'interactionSubmitting',
  'interactionSubmitFailed',
  'interactionApproved',
  'interactionDenied',
  'interactionCancelled',
  'interactionExpired',
  'interactionCancelling',
  'interactionSilenceHint',
  'interactionToolQueued',
  'interactionToolRunning',
  'interactionToolSuccess',
  'interactionToolError',
  'interactionToolCancelled',
  'interactionToolUnknown',
  'interactionSubmitAnswers',
  'interactionDeclineAnswers',
  'interactionOther',
  'interactionAnswerRequired',
  'interactionAnswerInvalid',
  'interactionUnsupportedField',
  'interactionEnterAnswer',
  'interactionSelectOne',
  'interactionSelectMany',
  'interactionChatAboutThis',
  'interactionSkipInterview',
  'interactionCancelRestarting',
  'interactionUsingSharedAgent',
  'interactionLaunchingAgent',
  'interactionAgentReady',
  'interactionRestoringSession',
  'interactionSavedSessionExpired',
  'interactionCreatingSession',
  'interactionSendingPrompt',
  'interactionNetworkRetry',
  'interactionNetworkRetryAttempt',
  'interactionNetworkRetryProgress',
  'interactionSessionExpired',
  'interactionAnswersSubmitted',
] as const;

describe('ACP interaction i18n', () => {
  it('defines decision-composer labels in every locale', () => {
    const localesDir = resolve(process.cwd(), 'src/i18n/locales');
    for (const fileName of readdirSync(localesDir).filter((name) => name.endsWith('.json'))) {
      const locale = JSON.parse(readFileSync(resolve(localesDir, fileName), 'utf8')) as {
        agentPage?: Record<string, unknown>;
      };
      for (const key of requiredKeys) {
        expect(locale.agentPage?.[key], `${fileName}: agentPage.${key}`).toEqual(
          expect.any(String),
        );
        expect(String(locale.agentPage?.[key] ?? '').trim(), `${fileName}: agentPage.${key}`).not.toBe('');
      }

      expect(locale.agentPage?.interactionNetworkRetry).not.toContain('{{attempt}}');
      expect(locale.agentPage?.interactionNetworkRetry).not.toContain('{{maximum}}');
      expect(locale.agentPage?.interactionNetworkRetryAttempt).toContain('{{attempt}}');
      expect(locale.agentPage?.interactionNetworkRetryAttempt).not.toContain('{{maximum}}');
      expect(locale.agentPage?.interactionNetworkRetryProgress).toContain('{{attempt}}');
      expect(locale.agentPage?.interactionNetworkRetryProgress).toContain('{{maximum}}');
      expect(locale.agentPage?.interactionUnsupportedField).toContain('{{type}}');
    }
  });
});
