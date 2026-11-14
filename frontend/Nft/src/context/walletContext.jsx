import { createContext, useContext, useState, useCallback, useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { setupWalletSelector } from "@near-wallet-selector/core";
import { setupModal } from "@near-wallet-selector/modal-ui";
import "@near-wallet-selector/modal-ui/styles.css";
import { EXPLORER_URL, HELPER_URL, NETWORK_ID, NODE_URL } from "../config/near";
import { supabase } from "../lib/supabase";
import { loginWithWallet, logoutUser } from "../services/auth";

const WalletContext = createContext(null);

export function WalletProvider({ children }) {
  const navigate = useNavigate();
  const [selector, setSelector] = useState(null);
  const [modal, setModal] = useState(null);
  const [accountId, setAccountId] = useState(null);
  const [isConnected, setIsConnected] = useState(false);
  const [loading, setLoading] = useState(false);
  const [isInitializing, setIsInitializing] = useState(true);
  const [user, setUser] = useState(null);
  const [profile, setProfile] = useState(null);
  const [balance, setBalance] = useState(0);
  const [error, setError] = useState(null);
  const [verifiedUntil, setVerifiedUntil] = useState(null);
  const selectorRef = useRef(null);
  const accountRef = useRef(null);
  const verifiedUserRef = useRef(null);
  const verifiedSessionRef = useRef(null);
  const authAbortRef = useRef(null);
  const retryAuthRef = useRef(null);

  const clearIdentity = useCallback(() => {
    verifiedUserRef.current = null;
    verifiedSessionRef.current = null;
    setUser(null); setProfile(null); setIsConnected(false); setVerifiedUntil(null);
    localStorage.removeItem("near_account");
  }, []);

  const refreshBalance = useCallback(async (targetAccountId) => {
    const acc = targetAccountId || accountRef.current;
    const currentSelector = selectorRef.current;
    if (!acc || !currentSelector) return 0;
    const primary = currentSelector.options?.network?.nodeUrl || NODE_URL;
    const endpoints = [...new Set([primary, "https://rpc.testnet.fastnear.com", "https://test.rpc.fastnear.com", "https://rpc.testnet.near.org"].filter(Boolean))];
    for (let i = 0; i < endpoints.length; i++) {
      const endpoint = endpoints[i];
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 6000);
        const response = await fetch(endpoint, {
          method: "POST", headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ jsonrpc: "2.0", id: "balance-" + acc, method: "query",
            params: { request_type: "view_account", finality: "final", account_id: acc } }),
          signal: controller.signal,
        });
        clearTimeout(timer);
        if (!response.ok) {
          if (i < endpoints.length - 1) continue;
          throw new Error("NEAR balance request failed");
        }
        const result = await response.json();
        if (result.error) {
          if (i < endpoints.length - 1) continue;
          throw new Error(result.error.message || "Balance query error");
        }
        const nearBalance = Number(result.result?.amount || "0") / 1e24;
        if (accountRef.current === acc) setBalance(nearBalance);
        return nearBalance;
      } catch {
        if (i < endpoints.length - 1) continue;
      }
    }
    if (accountRef.current === acc) setBalance(0);
    return 0;
  }, []);

  useEffect(() => {
    let disposed = false;
    let subscription;
    let lastAccount;
    let lastWallet;
    const init = async () => {
      try {
        // Wallet integrations are large and several carry optional Node-facing
        // provider code. Load them only when selector initialization begins so
        // the application shell is not forced into one multi-megabyte chunk.
        const [myNear, meteor, here, sender] = await Promise.all([
          import("@near-wallet-selector/my-near-wallet"),
          import("@near-wallet-selector/meteor-wallet"),
          import("@near-wallet-selector/here-wallet"),
          import("@near-wallet-selector/sender"),
        ]);
        if (disposed) return;
        const networkConfig = {
          networkId: NETWORK_ID,
          nodeUrl: NODE_URL,
          helperUrl: HELPER_URL,
          explorerUrl: EXPLORER_URL,
          indexerUrl: "https://test.api.fastnear.com/v0",
        };
        const currentSelector = await setupWalletSelector({
          network: networkConfig, debug: false,
          modules: [
            myNear.setupMyNearWallet(),
            meteor.setupMeteorWallet(),
            here.setupHereWallet(),
            sender.setupSender(),
          ],
        });
        if (disposed) return;
        const currentModal = setupModal(currentSelector, { contractId: import.meta.env.VITE_NFT_CONTRACT_ID || "" });
        selectorRef.current = currentSelector;
        setSelector(currentSelector); setModal(currentModal);

        const synchronize = async (state, force = false) => {
          const active = state.accounts.find((entry) => entry.active)?.accountId || null;
          if (disposed || (!force && active === lastAccount && state.selectedWalletId === lastWallet)) return;
          const previousAccount = accountRef.current;
          lastAccount = active;
          lastWallet = state.selectedWalletId;
          authAbortRef.current?.abort();
          const controller = new AbortController();
          authAbortRef.current = controller;
          accountRef.current = active;
          setAccountId(active); setBalance(0); clearIdentity(); setError(null);
          if (!active) {
            if (previousAccount) logoutUser(previousAccount).catch((failure) => {
              if (!disposed && !controller.signal.aborted) setError(failure.message);
            });
            setLoading(false); setIsInitializing(false);
            return;
          }
          setLoading(true);
          let redirecting = false;
          try {
            const result = await loginWithWallet(active, {
              selector: currentSelector, signal: controller.signal,
              previousAccount: previousAccount && previousAccount !== active ? previousAccount : undefined,
            });
            if (disposed || controller.signal.aborted) return;
            if (result.redirecting) { redirecting = true; return; }
            verifiedUserRef.current = result.user.id;
            verifiedSessionRef.current = result.sessionId;
            setUser(result.user); setProfile(result.profile); setVerifiedUntil(result.verifiedUntil);
            setIsConnected(true);
            localStorage.setItem("near_account", active);
            await refreshBalance(active);
          } catch (failure) {
            if (!disposed && !controller.signal.aborted) {
              clearIdentity();
              setError(failure.message);
            }
          } finally {
            if (!disposed && !controller.signal.aborted && !redirecting) {
              setLoading(false); setIsInitializing(false);
            }
          }
        };
        retryAuthRef.current = () => synchronize(currentSelector.store.getState(), true);
        subscription = currentSelector.store.observable.subscribe((state) => { void synchronize(state); });
        void synchronize(currentSelector.store.getState());
      } catch (failure) {
        if (!disposed) { setError(failure.message); setLoading(false); setIsInitializing(false); }
      }
    };
    void init();
    return () => {
      disposed = true;
      subscription?.unsubscribe();
      authAbortRef.current?.abort();
      retryAuthRef.current = null;
    };
  }, [clearIdentity, refreshBalance]);

  useEffect(() => {
    if (!verifiedUntil) return;
    const timer = setTimeout(() => {
      clearIdentity();
      setError("Wallet verification expired. Reconnect to sign in again.");
    }, Math.max(0, Date.parse(verifiedUntil) - Date.now()));
    return () => clearTimeout(timer);
  }, [verifiedUntil, clearIdentity]);

  useEffect(() => {
    const { data } = supabase.auth.onAuthStateChange((event, session) => {
      // Do not trust SIGNED_IN alone, or do async Auth calls inside this callback.
      let sessionId;
      try {
        const claims = session?.access_token?.split(".")[1].replace(/-/g, "+").replace(/_/g, "/");
        sessionId = JSON.parse(atob(claims)).session_id;
      } catch { /* Only compare against an already server-verified session ID. */ }
      if (verifiedUserRef.current && (event === "SIGNED_OUT" || session?.user?.id !== verifiedUserRef.current ||
          sessionId !== verifiedSessionRef.current)) {
        authAbortRef.current?.abort();
        clearIdentity();
        setError("Supabase session changed. Reconnect your wallet to verify it.");
      }
    });
    return () => data.subscription.unsubscribe();
  }, [clearIdentity]);

  const connectWallet = useCallback(async () => {
    setError(null);
    if (accountRef.current && !user && retryAuthRef.current) return retryAuthRef.current();
    if (!modal) return null;
    modal.show();
    return true;
  }, [modal, user]);

  const disconnectWallet = useCallback(async () => {
    authAbortRef.current?.abort();
    const previousAccount = accountRef.current;
    clearIdentity(); setLoading(true);
    try {
      // Revoke DB verification even if the wallet extension's signOut fails.
      await logoutUser(previousAccount);
    } catch (failure) { setError(failure.message); }
    try {
      if (selectorRef.current) await (await selectorRef.current.wallet()).signOut();
    } catch { setError("Marketplace signed out; disconnect the wallet in its extension if needed."); }
    finally {
      accountRef.current = null;
      setAccountId(null); setBalance(0); setLoading(false);
      navigate("/login", { replace: true });
    }
  }, [clearIdentity, navigate]);

  const value = {
    selector, modal, walletAddress: accountId, accountId, isConnected, loading, user, profile, balance, error,
    networkLabel: NETWORK_ID === "testnet" ? "NEAR Testnet" : "NEAR", isInitializing,
    networkHint: "This app uses NEAR Testnet. Make sure your wallet is connected to Testnet.",
    connectWallet, disconnectWallet, getWalletAddress: () => accountId, refreshBalance,
  };
  return <WalletContext.Provider value={value}>{children}</WalletContext.Provider>;
}

// The hook intentionally shares this module with its provider so they use the
// same private context instance.
// eslint-disable-next-line react-refresh/only-export-components
export function useWallet() {
  const context = useContext(WalletContext);
  if (!context) throw new Error("useWallet must be used within a WalletProvider");
  return context;
}
