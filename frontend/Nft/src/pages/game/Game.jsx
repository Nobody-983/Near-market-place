
import { useCallback, useEffect, useState } from "react";
import { Sun, Zap, Loader2, ArrowUpRight } from "lucide-react";
import { useNavigate } from "react-router-dom";

import { useWallet } from "../../context/walletContext";
import { supabase } from "../../lib/supabase";
import { validateNearAmount } from "../../lib/near/utils";
import {
  mintNftOnChain,
  transactionHash,
  waitForFinal,
  assertFinalSuccess,
  nftToken,
  requireConnectedAccount,
} from "../../lib/near/contract";
import {
  MINTING_ACTIONS_ENABLED,
  MINTING_UNAVAILABLE_MESSAGE,
} from "../../config/marketplace";

const MILESTONE_LEVELS = [
  {
    level: 1,
    tapsRequired: 500,
    reward: "Bronze NFT",
  },
  {
    level: 2,
    tapsRequired: 2000,
    reward: "Silver NFT",
  },
  {
    level: 3,
    tapsRequired: 5000,
    reward: "Gold NFT",
  },
  {
    level: 4,
    tapsRequired: 10000,
    reward: "Legendary Champion NFT",
  },
];

function Game() {
  const navigate = useNavigate();
  const { user, selector, walletAddress, isConnected } = useWallet();
  const userId = user?.id;

  const [tapCount, setTapCount] = useState(0);
  const [claimedRewards, setClaimedRewards] = useState([]);
  const [leaderboard, setLeaderboard] = useState([]);

  const [isLoading, setIsLoading] = useState(true);
  const [isTapping, setIsTapping] = useState(false);
  const [claimingLevel, setClaimingLevel] = useState(null);
  const [mintingRewardId, setMintingRewardId] = useState(null);
  const [mintStatus, setMintStatus] = useState("");
  const [mintError, setMintError] = useState("");

  // ================= LOAD GAME DATA =================

  const loadGameData = useCallback(async () => {
    if (!userId) {
      setTapCount(0);
      setClaimedRewards([]);
      setIsLoading(false);
      return;
    }

    setIsLoading(true);

    try {
      const [profileResult, rewardsResult] = await Promise.all([
        supabase
          .from("profiles")
          .select("tap_count")
          .eq("id", userId)
          .single(),

        supabase
          .from("reward_claims")
          .select(`
            id,
            reward_level,
            nft_id,
            nfts (
              id,
              name,
              description,
              image_url,
              category,
              price::text,
              currency,
              token_id,
              nft_contract_id,
              onchain_status,
              mint_tx_hash
            )
          `)
          .eq("user_id", userId)
          .order("reward_level", {
            ascending: true,
          }),
      ]);

      if (profileResult.error) {
        throw profileResult.error;
      }

      if (rewardsResult.error) {
        throw rewardsResult.error;
      }

      setTapCount(profileResult.data?.tap_count ?? 0);

      setClaimedRewards(
        (rewardsResult.data ?? []).map((reward) => ({
          id: reward.id,
          level: reward.reward_level,
          nftId: reward.nft_id,
          nft: reward.nfts ? {
            ...reward.nfts, price: validateNearAmount(reward.nfts.price),
          } : null,
        }))
      );
    } catch (error) {
      console.error("FAILED TO LOAD GAME DATA:", error);
    } finally {
      setIsLoading(false);
    }
  }, [userId]);

  // ================= LOAD LEADERBOARD =================

  const loadLeaderboard = useCallback(async () => {
    if (!userId) {
      setLeaderboard([]);
      return;
    }

    try {
      const { data, error } = await supabase
        .from("profiles")
        .select(
          "id, username, display_name, avatar_url, tap_count"
        )
        .order("tap_count", {
          ascending: false,
        })
        .limit(10);

      if (error) {
        throw error;
      }

      let players = data ?? [];

      // Make sure the current user is visible even
      // when they are outside the top 10.
      const currentUserIsIncluded = players.some(
        (player) => player.id === userId
      );

      if (!currentUserIsIncluded) {
        const { data: currentUser, error: currentUserError } =
          await supabase
            .from("profiles")
            .select(
              "id, username, display_name, avatar_url, tap_count"
            )
            .eq("id", userId)
            .maybeSingle();

        if (currentUserError) {
          throw currentUserError;
        }

        if (currentUser) {
          players = [...players, currentUser];
        }
      }

      // Calculate the actual rank based on all players.
      const { data: allProfiles, error: rankError } =
        await supabase
          .from("profiles")
          .select("id, tap_count")
          .order("tap_count", {
            ascending: false,
          });

      if (rankError) {
        throw rankError;
      }

      const rankedPlayers = allProfiles ?? [];

      players = players.map((player) => {
        const rank =
          rankedPlayers.findIndex(
            (rankedPlayer) => rankedPlayer.id === player.id
          ) + 1;

        return {
          ...player,
          rank,
        };
      });

      setLeaderboard(players);
    } catch (error) {
      console.error(
        "FAILED TO LOAD LEADERBOARD:",
        error
      );
    }
  }, [userId]);

  // ================= INITIAL LOAD =================

  useEffect(() => {
    void Promise.resolve().then(() => Promise.all([loadGameData(), loadLeaderboard()]));
  }, [loadGameData, loadLeaderboard]);

  // ================= TAP =================

  const handleTap = async () => {
    if (!user?.id || isTapping) {
      return;
    }

    setIsTapping(true);

    const previousTapCount = tapCount;
    const newTapCount = previousTapCount + 1;

    // Update UI immediately.
    setTapCount(newTapCount);

    try {
      const { error } = await supabase
        .from("profiles")
        .update({
          tap_count: newTapCount,
          updated_at: new Date().toISOString(),
        })
        .eq("id", user.id);

      if (error) {
        throw error;
      }

      await loadLeaderboard();
    } catch (error) {
      console.error("FAILED TO SAVE TAP:", error);

      // Roll back the UI if the database update fails.
      setTapCount(previousTapCount);
    } finally {
      setIsTapping(false);
    }
  };

  // ================= REWARD HELPERS =================

  const getClaimedReward = (level) => {
    return claimedRewards.find(
      (reward) => reward.level === level
    );
  };

  const hasClaimedReward = (level) => {
    return Boolean(getClaimedReward(level));
  };

  // ================= CLAIM REWARD =================

  const claimReward = async (milestone) => {
    if (!user?.id) {
      return;
    }

    if (tapCount < milestone.tapsRequired) {
      return;
    }

    if (hasClaimedReward(milestone.level)) {
      return;
    }

    setClaimingLevel(milestone.level);

    try {
      const { data, error } = await supabase.rpc(
        "claim_game_reward",
        {
          p_reward_level: milestone.level,
        }
      );

      if (error) {
        throw error;
      }

      console.log("REWARD CLAIMED:", data);

      // Reload everything from Supabase so the NFT
      // created by the RPC is immediately available.
      await loadGameData();
    } catch (error) {
      console.error(
        "FAILED TO CLAIM REWARD:",
        error
      );

      alert(
        error?.message ||
          "Failed to claim this reward. Please try again."
      );
    } finally {
      setClaimingLevel(null);
    }
  };

  // ================= MINT REWARD TO ON-CHAIN NEAR NFT =================

  const handleMintReward = async (nft) => {
    if (!MINTING_ACTIONS_ENABLED) {
      setMintError(MINTING_UNAVAILABLE_MESSAGE);
      return;
    }
    if (!isConnected || !selector || !walletAddress) {
      setMintError("Connect the configured NEAR Testnet mint-authority wallet first.");
      return;
    }
    if (!nft?.id || nft.token_id || nft.nft_contract_id || mintingRewardId) return;

    const tokenId = `catalog-${nft.id}`;
    const wallet = { selector, accountId: walletAddress };
    setMintingRewardId(nft.id);
    setMintError("");
    setMintStatus("Submitting on-chain mint to NEAR Testnet...");

    try {
      await requireConnectedAccount(walletAddress, wallet);
      const result = await mintNftOnChain({
        tokenId,
        ownerId: walletAddress,
        wallet,
        metadata: {
          title: nft.name,
          description: nft.description || null,
          media: nft.image_url,
          media_hash: null,
          copies: "1",
          issued_at: null,
          expires_at: null,
          starts_at: null,
          updated_at: null,
          extra: JSON.stringify({ catalog_id: nft.id, category: nft.category, source: "game_reward" }),
          reference: null,
          reference_hash: null,
        },
      });
      const txHash = transactionHash(result);
      setMintStatus("Waiting for on-chain block finality...");
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Game reward NFT mint");
      await requireConnectedAccount(walletAddress, wallet);
      const token = await nftToken(tokenId, wallet);
      if (!token || token.owner_id !== walletAddress || token.token_id !== tokenId) {
        throw new Error("The finalized mint could not be verified against NFT contract state.");
      }
      setMintStatus(`"${nft.name}" was minted on NEAR! The trusted indexer is synchronizing the catalog record.`);
      await loadGameData();
    } catch (err) {
      console.error("MINT REWARD ERROR:", err);
      setMintError(err?.message || "Failed to mint reward NFT on NEAR.");
    } finally {
      setMintingRewardId(null);
    }
  };

  // ================= PROGRESS =================

  const getProgress = (milestone) => {
    if (tapCount >= milestone.tapsRequired) {
      return 100;
    }

    return Math.min(
      (tapCount / milestone.tapsRequired) * 100,
      100
    );
  };

  const getRemainingTaps = (milestone) => {
    return Math.max(
      milestone.tapsRequired - tapCount,
      0
    );
  };

  // ================= LOADING =================

  if (isLoading) {
    return (
      <div className="flex min-h-[60vh] items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin" />
      </div>
    );
  }

  // ================= PAGE =================

  return (
    <div className="space-y-8 pb-10">
      {/* Header */}

      <div>
        <div className="flex items-center gap-3">
          <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-yellow-400/10">
            <Sun className="h-6 w-6 text-yellow-400" />
          </div>

          <div>
            <h1 className="text-2xl font-bold">
              NEAR Tap Game
            </h1>

            <p className="text-sm text-gray-500">
              Tap the coin, reach milestones, and earn NFTs.
            </p>
          </div>
        </div>
      </div>

      {/* Tap Game */}

      <section className="rounded-2xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="text-center">
          <p className="text-sm text-gray-500">
            Your taps
          </p>

          <h2 className="mt-2 text-4xl font-bold">
            {tapCount.toLocaleString()}
          </h2>

          <p className="mt-2 text-sm text-gray-500">
            Keep tapping to unlock NFT rewards.
          </p>

          <button
            type="button"
            onClick={handleTap}
            disabled={isTapping}
            className="mx-auto mt-8 flex h-36 w-36 items-center justify-center rounded-full border-8 border-yellow-400/30 bg-yellow-400/10 shadow-lg transition-transform duration-100 active:scale-90 disabled:opacity-70"
          >
            {isTapping ? (
              <Loader2 className="h-12 w-12 animate-spin text-yellow-400" />
            ) : (
              <Sun className="h-16 w-16 text-yellow-400" />
            )}
          </button>

          <p className="mt-5 text-sm text-gray-500">
            Tap the NEAR coin
          </p>
        </div>
      </section>

      {/* Mint Status / Error Alerts */}
      {mintStatus && (
        <div className="rounded-xl border border-green-500/20 bg-green-500/10 p-4 text-sm text-green-400">
          {mintStatus}
        </div>
      )}
      {mintError && (
        <div className="rounded-xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-400">
          {mintError}
        </div>
      )}

      {/* NFT Milestones */}

      <section>
        <div className="mb-4">
          <h2 className="text-xl font-semibold">
            NFT Milestones
          </h2>

          <p className="text-sm text-gray-500">
            Reach the required number of taps to unlock each NFT.
          </p>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          {MILESTONE_LEVELS.map((milestone) => {
            const claimedReward = getClaimedReward(
              milestone.level
            );

            const claimed = Boolean(claimedReward);

            const unlocked =
              tapCount >= milestone.tapsRequired;

            const progress =
              getProgress(milestone);

            const remaining =
              getRemainingTaps(milestone);

            return (
              <div
                key={milestone.level}
                className="rounded-2xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900"
              >
                <div className="flex items-center justify-between">
                  <span className="text-sm text-gray-500">
                    Level {milestone.level}
                  </span>

                  {claimed && (
                    <span className="rounded-full bg-green-500/10 px-3 py-1 text-xs font-medium text-green-500">
                      Claimed
                    </span>
                  )}

                  {!claimed && unlocked && (
                    <span className="rounded-full bg-yellow-400/10 px-3 py-1 text-xs font-medium text-yellow-500">
                      Unlocked
                    </span>
                  )}
                </div>

                <h3 className="mt-4 text-lg font-semibold">
                  {milestone.reward}
                </h3>

                <p className="mt-1 text-sm text-gray-500">
                  {milestone.tapsRequired.toLocaleString()} taps
                </p>

                {/* Progress */}

                <div className="mt-5">
                  <div className="mb-2 flex justify-between text-xs text-gray-500">
                    <span>
                      {tapCount.toLocaleString()} taps
                    </span>

                    <span>
                      {milestone.tapsRequired.toLocaleString()}
                    </span>
                  </div>

                  <div className="h-2 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
                    <div
                      className="h-full rounded-full bg-yellow-400 transition-all duration-300"
                      style={{
                        width: `${progress}%`,
                      }}
                    />
                  </div>
                </div>

                {/* Claim */}

                {!claimed && unlocked && (
                  <button
                    type="button"
                    onClick={() =>
                      claimReward(milestone)
                    }
                    disabled={
                      claimingLevel === milestone.level
                    }
                    className="mt-5 w-full rounded-xl bg-yellow-400 px-4 py-3 text-sm font-semibold text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {claimingLevel === milestone.level ? (
                      <span className="flex items-center justify-center gap-2">
                        <Loader2 className="h-4 w-4 animate-spin" />
                        Claiming...
                      </span>
                    ) : (
                      "Claim NFT"
                    )}
                  </button>
                )}

                {/* Locked */}

                {!claimed && !unlocked && (
                  <p className="mt-5 text-center text-xs text-gray-500">
                    {remaining.toLocaleString()} more taps
                    to unlock
                  </p>
                )}

                {/* Claimed NFT */}

                {claimed && claimedReward.nft && (
                  <div className="mt-5 overflow-hidden rounded-xl border border-gray-200 dark:border-gray-700">
                    <img
                      src={claimedReward.nft.image_url}
                      alt={claimedReward.nft.name}
                      className="h-48 w-full object-cover"
                    />

                    <div className="p-4">
                      <p className="text-sm font-semibold">
                        {claimedReward.nft.name}
                      </p>

                      {claimedReward.nft.description && (
                        <p className="mt-1 text-xs text-gray-500">
                          {claimedReward.nft.description}
                        </p>
                      )}

                      {claimedReward.nft.token_id ? (
                        <div className="mt-3 space-y-2">
                          <span className="inline-flex items-center gap-1.5 rounded-full bg-green-500/10 px-2.5 py-1 text-xs font-semibold text-green-500">
                            ✓ Genuine On-Chain NFT
                          </span>
                          <p className="truncate font-mono text-[11px] text-gray-400">
                            {claimedReward.nft.token_id}
                          </p>
                          <button
                            type="button"
                            onClick={() => navigate(`/nft/${claimedReward.nft.id}`)}
                            className="mt-2 flex w-full items-center justify-center gap-1.5 rounded-lg bg-purple-600 px-3 py-2 text-xs font-semibold text-white transition hover:bg-purple-700"
                          >
                            <span>List on Marketplace</span>
                            <ArrowUpRight className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      ) : (
                        <div className="mt-3 space-y-2">
                          <span className="inline-flex items-center gap-1 rounded-full bg-yellow-400/10 px-2.5 py-1 text-xs font-medium text-yellow-500">
                            Claimed Collectible
                          </span>
                          <button
                            type="button"
                            onClick={() => handleMintReward(claimedReward.nft)}
                            disabled={mintingRewardId === claimedReward.nft.id || !MINTING_ACTIONS_ENABLED}
                            className="mt-2 w-full rounded-lg bg-yellow-400 px-3 py-2 text-xs font-semibold text-black transition hover:opacity-90 disabled:opacity-50"
                          >
                            {mintingRewardId === claimedReward.nft.id ? (
                              <span className="flex items-center justify-center gap-1.5">
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                Minting on NEAR...
                              </span>
                            ) : (
                              "Mint Genuine NFT on NEAR"
                            )}
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                )}

                {claimed && !claimedReward.nft && (
                  <p className="mt-5 text-center text-xs text-yellow-500">
                    Reward claimed, but NFT details are unavailable.
                  </p>
                )}
              </div>
            );
          })}
        </div>
      </section>

      {/* Your Game NFTs */}

      {claimedRewards.some((reward) => reward.nft) && (
        <section>
          <div className="mb-4">
            <h2 className="text-xl font-semibold">
              Your Game NFTs
            </h2>

            <p className="text-sm text-gray-500">
              NFTs you earned from the NEAR Tap Game.
            </p>
          </div>

          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {claimedRewards
              .filter((reward) => reward.nft)
              .map((reward) => (
                <div
                  key={reward.id}
                  className="overflow-hidden rounded-2xl border border-gray-200 bg-white shadow-sm dark:border-gray-800 dark:bg-gray-900"
                >
                  <img
                    src={reward.nft.image_url}
                    alt={reward.nft.name}
                    className="h-56 w-full object-cover"
                  />

                  <div className="p-5">
                    <div className="flex items-center justify-between gap-3">
                      <h3 className="font-semibold">
                        {reward.nft.name}
                      </h3>

                      <span className="rounded-full bg-yellow-400/10 px-2.5 py-1 text-xs text-yellow-500">
                        Level {reward.level}
                      </span>
                    </div>

                    {reward.nft.description && (
                      <p className="mt-2 text-sm text-gray-500">
                        {reward.nft.description}
                      </p>
                    )}

                    <div className="mt-4 flex flex-col gap-2">
                      {reward.nft.token_id ? (
                        <>
                          <div className="flex items-center justify-between text-xs">
                            <span className="font-semibold text-green-500">
                              ✓ Verified On-Chain NFT
                            </span>
                            <span className="truncate font-mono text-gray-400 max-w-[120px]">
                              {reward.nft.token_id}
                            </span>
                          </div>
                          <button
                            type="button"
                            onClick={() => navigate(`/nft/${reward.nft.id}`)}
                            className="flex items-center justify-center gap-1.5 rounded-lg bg-purple-600 px-3 py-2 text-xs font-semibold text-white transition hover:bg-purple-700"
                          >
                            <span>View / List for Sale</span>
                            <ArrowUpRight className="h-3.5 w-3.5" />
                          </button>
                        </>
                      ) : (
                        <>
                          <div className="flex items-center justify-between text-xs">
                            <span className="text-yellow-500">
                              Claimed Collectible
                            </span>
                            <span className="text-gray-400">
                              Unminted
                            </span>
                          </div>
                          <button
                            type="button"
                            onClick={() => handleMintReward(reward.nft)}
                            disabled={mintingRewardId === reward.nft.id || !MINTING_ACTIONS_ENABLED}
                            className="flex items-center justify-center gap-1.5 rounded-lg bg-yellow-400 px-3 py-2 text-xs font-semibold text-black transition hover:opacity-90 disabled:opacity-50"
                          >
                            {mintingRewardId === reward.nft.id ? (
                              <>
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                <span>Minting on NEAR...</span>
                              </>
                            ) : (
                              <span>Mint Genuine NFT</span>
                            )}
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                </div>
              ))}
          </div>
        </section>
      )}

      {/* Leaderboard */}

      <section>
        <div className="mb-4">
          <h2 className="text-xl font-semibold">
            Leaderboard
          </h2>

          <p className="text-sm text-gray-500">
            Top players by total taps.
          </p>
        </div>

        <div className="overflow-hidden rounded-2xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
          <div className="divide-y divide-gray-200 dark:divide-gray-800">
            {leaderboard.length === 0 ? (
              <div className="px-5 py-8 text-center text-sm text-gray-500">
                No players yet.
              </div>
            ) : (
              leaderboard.map((player) => {
                const playerName =
                  player.username ||
                  player.display_name ||
                  "NEAR Player";

                const isCurrentUser =
                  player.id === user?.id;

                return (
                  <div
                    key={player.id}
                    className={`flex items-center justify-between px-5 py-4 ${
                      isCurrentUser
                        ? "bg-yellow-400/5"
                        : ""
                    }`}
                  >
                    <div className="flex items-center gap-4">
                      <div className="flex h-9 w-9 items-center justify-center rounded-full bg-gray-100 text-sm font-semibold dark:bg-gray-800">
                        {player.rank}
                      </div>

                      <div>
                        <p className="font-medium">
                          {playerName}
                        </p>

                        {isCurrentUser && (
                          <p className="text-xs text-yellow-500">
                            You
                          </p>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 text-sm font-semibold">
                      <Zap className="h-4 w-4 text-yellow-400" />

                      {(player.tap_count ?? 0).toLocaleString()}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </section>
    </div>
  );
}

export default Game;
