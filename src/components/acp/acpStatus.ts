import {
  ACP_HOST_STATUS,
  ACP_STATUS_CANCELLING,
  ACP_STATUS_FIRST_OUTPUT_SILENCE,
} from '@/stores/acpStore';

type AcpStatusTranslator = (
  key: string,
  values?: Record<string, string | number>,
) => string;

interface GrokRetryStatusPayload {
  attempt?: number;
  maximum?: number;
  detail?: string;
}

const ACP_STATUS_TRANSLATIONS: Readonly<Record<string, string>> = {
  [ACP_STATUS_FIRST_OUTPUT_SILENCE]: 'agentPage.interactionSilenceHint',
  [ACP_STATUS_CANCELLING]: 'agentPage.interactionCancelling',
  [ACP_HOST_STATUS.cancelRestarting]: 'agentPage.interactionCancelRestarting',
  [ACP_HOST_STATUS.usingSharedAgent]: 'agentPage.interactionUsingSharedAgent',
  [ACP_HOST_STATUS.launchingAgent]: 'agentPage.interactionLaunchingAgent',
  [ACP_HOST_STATUS.agentReady]: 'agentPage.interactionAgentReady',
  [ACP_HOST_STATUS.restoringSession]: 'agentPage.interactionRestoringSession',
  [ACP_HOST_STATUS.savedSessionExpired]: 'agentPage.interactionSavedSessionExpired',
  [ACP_HOST_STATUS.creatingSession]: 'agentPage.interactionCreatingSession',
  [ACP_HOST_STATUS.sendingPrompt]: 'agentPage.interactionSendingPrompt',
  [ACP_HOST_STATUS.sessionExpired]: 'agentPage.interactionSessionExpired',
};

export function localizeAcpStatus(
  status: string | undefined,
  translate: AcpStatusTranslator,
): string {
  if (!status) return '';
  const localized = Object.prototype.hasOwnProperty.call(ACP_STATUS_TRANSLATIONS, status)
    ? ACP_STATUS_TRANSLATIONS[status]
    : undefined;
  if (localized) return translate(localized);
  if (status.startsWith(ACP_HOST_STATUS.grokRetry)) {
    try {
      const payload = JSON.parse(
        status.slice(ACP_HOST_STATUS.grokRetry.length),
      ) as GrokRetryStatusPayload;
      const values = { attempt: payload.attempt ?? 0, maximum: payload.maximum ?? 0 };
      const progress = typeof payload.attempt === 'number'
        ? typeof payload.maximum === 'number'
          ? translate(
            'agentPage.interactionNetworkRetryProgress',
            values,
          )
          : translate(
            'agentPage.interactionNetworkRetryAttempt',
            values,
          )
        : translate('agentPage.interactionNetworkRetry');
      return payload.detail ? `${progress}: ${payload.detail}` : progress;
    } catch {
      return status;
    }
  }
  return status;
}
