import { useAirdropTokensRefresh } from './stateHelpers/useAirdropTokensRefresh';
import { useAirdropUserPointsListener } from './stateHelpers/useAirdropUserPointsListener';
import { useGetAirdropUserDetails } from './stateHelpers/useGetAirdropUserDetails';
import { useGetReferralQuestPoints } from './stateHelpers/useGetReferralQuestPoints';
import { useGetRustInMemoryConfig } from './stateHelpers/useGetRustInMemoryConfig';

export const useAirdropSyncState = () => {
    useGetRustInMemoryConfig();
    useAirdropTokensRefresh();
    useGetAirdropUserDetails();
    useAirdropUserPointsListener();
    useGetReferralQuestPoints();
};
