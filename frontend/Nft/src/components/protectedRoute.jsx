
import { useWallet } from "../context/walletContext";

function ProtectedRoute({ children }) {
  const {
    user,
    isInitializing,
  } = useWallet();

  // Wait until the wallet/Supabase session
  // has finished restoring.
  if (isInitializing) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="text-center">
          <div className="mb-3 text-lg font-medium">
            Loading...
          </div>

          <div className="text-sm opacity-60">
            Restoring your wallet session
          </div>
        </div>
      </div>
    );
  }

  // Allow display-only exploration in preview mode when no wallet is connected
  if (!user) {
    return (
      <div className="w-full">
        <div className="bg-gradient-to-r from-purple-950/80 via-zinc-900 to-emerald-950/80 border-b border-white/10 px-4 py-2 text-xs text-center text-gray-300 flex flex-wrap items-center justify-center gap-2">
          <span className="px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-300 font-semibold border border-yellow-500/30">
            Preview Mode
          </span>
          <span>Wallet not connected. Exploring display UI with preview data.</span>
          <a href="/login" className="text-[#00EC97] hover:underline font-medium ml-1">Connect NEAR Wallet</a>
        </div>
        {children}
      </div>
    );
  }

  // Authenticated user can access the protected page.
  return children;
}

export default ProtectedRoute;
