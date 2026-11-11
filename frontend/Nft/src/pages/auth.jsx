import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { motion } from "framer-motion";
import { Wallet, ArrowRight, Shield, Sparkles } from "lucide-react";
import { useWallet } from "../context/walletContext";
import ThemeToggle from "../components/themeToggle";

function Auth() {
  const navigate = useNavigate();
  const { connectWallet, isConnected, loading, error, networkLabel, networkHint } = useWallet();
  const [localError, setLocalError] = useState("");

  useEffect(() => {
    if (isConnected) {
      navigate("/dashboard");
    }
  }, [isConnected, navigate]);

  const handleConnect = async () => {
    setLocalError("");
    try {
      await connectWallet();
    } catch (err) {
      setLocalError(err?.message || "Failed to connect NEAR wallet");
    }
  };

  return (
    <div className="min-h-screen bg-black text-white flex items-center justify-center px-4">
      <ThemeToggle />
      <div className="max-w-md w-full space-y-8">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6 }}
          className="text-center"
        >
          <div className="flex justify-center mb-6">
            <div className="w-16 h-16 bg-gradient-to-br from-[#00EC97] to-[#00C781] rounded-2xl flex items-center justify-center">
              <Sparkles className="w-8 h-8 text-black" />
            </div>
          </div>
          <h1 className="text-4xl font-bold mb-2">NEAR NFT</h1>
          <p className="text-gray-400 text-lg">Discover · Create · Trade</p>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.1 }}
          className="bg-zinc-900 rounded-2xl p-8 border border-zinc-800"
        >
          <div className="space-y-6">
            <div className="text-center">
              <h2 className="text-xl font-semibold mb-2">Connect Your Wallet</h2>
              <p className="text-gray-400 text-sm">
                Connect with your NEAR wallet to access the marketplace
              </p>
            </div>

            <button
              onClick={handleConnect}
              disabled={loading}
              className="w-full bg-gradient-to-r from-[#00EC97] to-[#00C781] text-black font-semibold py-3 px-6 rounded-xl hover:shadow-lg hover:shadow-[#00EC97]/20 transition-all duration-200 flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Wallet className="w-5 h-5" />
              {loading ? "Connecting..." : "Connect NEAR Wallet"}
              <ArrowRight className="w-4 h-4" />
            </button>

            <button
              type="button"
              onClick={() => navigate("/marketplace")}
              className="w-full bg-white/[0.05] hover:bg-white/[0.09] text-gray-300 font-medium py-3 px-6 rounded-xl transition-all duration-200 flex items-center justify-center gap-2 border border-white/10 text-sm"
            >
              Explore Marketplace (Preview Mode)
              <ArrowRight className="w-4 h-4 text-gray-400" />
            </button>

            {(error || localError) && (
              <div className="p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-sm">
                {error || localError}
              </div>
            )}

            <div className="flex items-start gap-3 p-3 bg-[#00EC97]/5 border border-[#00EC97]/20 rounded-lg">
              <Shield className="w-5 h-5 text-[#00EC97] mt-0.5 flex-shrink-0" />
              <div className="text-xs text-gray-300">
                <p className="font-medium text-[#00EC97] mb-1">{networkLabel}</p>
                <p>{networkHint}</p>
              </div>
            </div>
          </div>
        </motion.div>
      </div>
    </div>
  );
}

export default Auth;
