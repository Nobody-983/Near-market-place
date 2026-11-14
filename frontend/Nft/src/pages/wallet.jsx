import { useCallback, useEffect, useState } from "react";
import {
  Wallet as WalletIcon,
  Copy,
  Check,
  Loader2,
  RefreshCw,
} from "lucide-react";

import { useWallet } from "../context/walletContext";
import { shortenAccountId, yoctoToNear } from "../lib/near/utils";
import {
  marketplaceBuyerRefund,
  marketplaceFeeProceeds,
  marketplaceSellerProceeds,
  marketplaceStorageBalance,
  marketplaceTransactions,
  requireConnectedAccount,
  transactionHash,
  waitForFinal,
  withdrawFeesOnChain,
  withdrawProceedsOnChain,
  withdrawRefundOnChain,
} from "../lib/near/contract.js";

function Wallet() {
  const {
    walletAddress,
    isConnected,
    loading: walletLoading,
    connectWallet,
    disconnectWallet,
    profile,
    balance,
    error,
    refreshBalance,
    networkHint,
    selector,
  } = useWallet();

  const [copied, setCopied] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [marketplace, setMarketplace] = useState(null);
  const [marketplaceError, setMarketplaceError] = useState("");
  const [withdrawing, setWithdrawing] = useState("");

  const refreshMarketplace = useCallback(async () => {
    if (!walletAddress || !isConnected || !selector) return;
    try {
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const [sellerProceeds, feeProceeds, buyerRefund, storage, transactions] = await Promise.all([
        marketplaceSellerProceeds(walletAddress, wallet),
        marketplaceFeeProceeds(walletAddress, wallet),
        marketplaceBuyerRefund(walletAddress, wallet),
        marketplaceStorageBalance(walletAddress, wallet),
        marketplaceTransactions(wallet, "0", 50),
      ]);
      await requireConnectedAccount(walletAddress, wallet);
      setMarketplace({
        sellerProceeds, feeProceeds, buyerRefund, storage,
        transactions: (transactions || []).filter((item) =>
          item.buyer === walletAddress || item.seller === walletAddress || item.fee_recipient === walletAddress
        ).reverse(),
      });
      setMarketplaceError("");
    } catch (err) {
      setMarketplace(null);
      setMarketplaceError(err?.message || "Marketplace contract is not available yet.");
    }
  }, [isConnected, selector, walletAddress]);

  useEffect(() => {
    if (!walletAddress) return undefined;
    const timer = window.setTimeout(() => {
      void refreshBalance(walletAddress);
      void refreshMarketplace();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [walletAddress, refreshBalance, refreshMarketplace]);

  const handleCopy = async () => {
    if (!walletAddress) return;
    try {
      await navigator.clipboard.writeText(walletAddress);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("COPY WALLET ERROR:", err);
    }
  };

  const handleManualRefresh = async () => {
    if (!walletAddress || refreshing) return;
    setRefreshing(true);
    try {
      await Promise.all([refreshBalance(walletAddress), refreshMarketplace()]);
    } finally {
      setTimeout(() => setRefreshing(false), 600);
    }
  };

  const withdrawMarketplaceBalance = async (kind, amount, saleId = undefined) => {
    if (!selector || !walletAddress || withdrawing) return;
    setWithdrawing(kind === "refund" ? saleId : kind);
    try {
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const result = kind === "seller"
        ? await withdrawProceedsOnChain(amount, wallet)
        : kind === "fee"
          ? await withdrawFeesOnChain(amount, wallet)
          : await withdrawRefundOnChain(saleId, wallet);
      const txHash = transactionHash(result);
      if (!txHash) throw new Error("Wallet did not return a withdrawal transaction hash.");
      const finalResult = await waitForFinal(result || txHash, walletAddress, wallet);
      if (finalResult?.status?.Failure) throw new Error("Marketplace withdrawal failed.");
      await requireConnectedAccount(walletAddress, wallet);
      await Promise.all([refreshBalance(walletAddress), refreshMarketplace()]);
    } catch (err) {
      setMarketplaceError(err?.message || "Unable to withdraw marketplace balance.");
    } finally {
      setWithdrawing("");
    }
  };

  const shortWalletAddress = walletAddress ? shortenAccountId(walletAddress) : "No wallet connected";

  return (
    <div className="min-h-screen bg-black px-4 py-6 text-white sm:px-6 lg:px-8">
      <div className="mb-8">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="text-3xl font-bold tracking-tight">Wallet</h1>
          <span className="rounded-full border border-[#00EC97]/20 bg-[#00EC97]/10 px-3 py-1 text-xs font-semibold text-[#00EC97]">
            TESTNET
          </span>
        </div>
        <p className="mt-1 text-sm text-gray-400">
          Manage your NEAR Testnet wallet, balance and transactions.
        </p>
      </div>

      {!isConnected ? (
        <div className="mx-auto max-w-md rounded-2xl border border-zinc-800 bg-zinc-900/50 p-8 text-center">
          <WalletIcon className="mx-auto mb-4 h-12 w-12 text-[#00EC97]" />
          <h2 className="mb-2 text-xl font-semibold">Connect Your NEAR Wallet</h2>
          <p className="mb-6 text-sm text-gray-400">
            Connect your NEAR wallet to view your balance and make transactions
          </p>
          <button
            onClick={connectWallet}
            disabled={walletLoading}
            className="w-full rounded-xl bg-gradient-to-r from-[#00EC97] to-[#00C781] px-6 py-3 font-semibold text-black transition-all hover:shadow-lg hover:shadow-[#00EC97]/20 disabled:opacity-50"
          >
            {walletLoading ? <Loader2 className="mx-auto h-5 w-5 animate-spin" /> : "Connect Wallet"}
          </button>
          {error && <p className="mt-4 text-sm text-red-400">{error}</p>}
        </div>
      ) : (
        <div className="space-y-6">
          <div className="rounded-2xl border border-zinc-800 bg-gradient-to-br from-zinc-900 to-black p-6">
            <div className="mb-4 flex items-start justify-between">
              <div>
                <p className="mb-1 text-sm text-gray-400">Account ID</p>
                <div className="flex items-center gap-2">
                  <p className="font-mono text-lg">{shortWalletAddress}</p>
                  <button onClick={handleCopy} className="rounded-lg p-1 hover:bg-zinc-800">
                    {copied ? <Check className="h-4 w-4 text-[#00EC97]" /> : <Copy className="h-4 w-4" />}
                  </button>
                </div>
                {profile?.display_name && <p className="mt-1 text-sm text-gray-400">{profile.display_name}</p>}
              </div>
              <button
                onClick={disconnectWallet}
                className="rounded-lg border border-zinc-700 px-4 py-2 text-sm hover:bg-zinc-800"
              >
                Disconnect
              </button>
            </div>

            <div className="mb-4">
              <p className="mb-1 text-sm text-gray-400">Balance</p>
              <div className="flex items-baseline gap-2">
                <p className="text-3xl font-bold">{balance.toFixed(4)}</p>
                <p className="text-lg text-gray-400">NEAR</p>
              </div>
            </div>

            <button
              onClick={handleManualRefresh}
              disabled={refreshing}
              className="flex items-center gap-2 rounded-lg border border-zinc-700 px-4 py-2 text-sm hover:bg-zinc-800 disabled:opacity-50"
            >
              <RefreshCw className={`h-4 w-4 ${refreshing ? "animate-spin" : ""}`} />
              Refresh Balance
            </button>
          </div>

          <div className="rounded-2xl border border-zinc-800 bg-zinc-900/50 p-6">
            <div className="mb-4 flex items-center justify-between gap-3">
              <div>
                <h3 className="text-lg font-semibold">Marketplace balances</h3>
                <p className="text-sm text-gray-400">On-chain proceeds, fees, refunds and storage credit.</p>
              </div>
              <button onClick={refreshMarketplace} className="rounded-lg border border-zinc-700 px-3 py-2 text-sm hover:bg-zinc-800">
                Refresh
              </button>
            </div>
            {marketplaceError && <p className="mb-4 text-sm text-amber-300">{marketplaceError}</p>}
            {marketplace && (
              <>
                <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  {[
                    ["Seller proceeds", marketplace.sellerProceeds, "seller"],
                    ["Marketplace fees", marketplace.feeProceeds, "fee"],
                    ["Buyer refunds", marketplace.buyerRefund, null],
                    ["Storage available", marketplace.storage?.available || "0", null],
                  ].map(([label, amount, kind]) => (
                    <div key={label} className="rounded-xl border border-zinc-800 bg-black/40 p-4">
                      <p className="text-xs text-gray-400">{label}</p>
                      <p className="mt-1 break-all font-mono text-sm">{yoctoToNear(amount || "0")} NEAR</p>
                      {kind && BigInt(amount || "0") > 0n && (
                        <button
                          onClick={() => withdrawMarketplaceBalance(kind, amount)}
                          disabled={Boolean(withdrawing)}
                          className="mt-3 rounded-lg border border-[#00EC97]/40 px-3 py-1 text-xs text-[#00EC97] disabled:opacity-50"
                        >
                          Withdraw
                        </button>
                      )}
                    </div>
                  ))}
                </div>
                <div className="mt-5">
                  <h4 className="mb-2 text-sm font-semibold">Recent on-chain settlements</h4>
                  {marketplace.transactions.length === 0 ? (
                    <p className="text-sm text-gray-500">No marketplace transactions found for this account.</p>
                  ) : (
                    <div className="space-y-2">
                      {marketplace.transactions.slice(0, 10).map((item) => (
                        <div key={item.sale_id} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-zinc-800 bg-black/30 p-3 text-sm">
                          <div>
                            <p className="font-medium">{item.token_id}</p>
                            <p className="text-xs text-gray-500">{item.status} · {yoctoToNear(item.price)} NEAR</p>
                          </div>
                          {item.status === "refund_available" && item.buyer === walletAddress && (
                            <button
                              onClick={() => withdrawMarketplaceBalance("refund", item.price, item.sale_id)}
                              disabled={Boolean(withdrawing)}
                              className="rounded-lg border border-amber-400/40 px-3 py-1 text-xs text-amber-300 disabled:opacity-50"
                            >
                              Claim refund
                            </button>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </>
            )}
          </div>

          <div className="rounded-2xl border border-zinc-800 bg-zinc-900/50 p-6">
            <h3 className="mb-2 text-lg font-semibold">NEAR Testnet</h3>
            <p className="text-sm text-gray-400">{networkHint}</p>
          </div>
        </div>
      )}
    </div>
  );
}

export default Wallet;
