import { BrowserRouter, Routes, Route } from "react-router-dom";

import { WalletProvider } from "./context/walletContext";
import { ThemeProvider } from "./context/themeContext";

import ProtectedRoute from "./components/protectedRoute";

import DashboardLayout from "./components/dashboardlayout";

import NFTDetails from "./pages/nftDetails";

import Dashboard from "./pages/dashboard";
import Marketplace from "./pages/market";
import Wallet from "./pages/wallet";
import Account from "./pages/Account";
import Auth from "./pages/auth";
import CreateNFT from "./pages/createNft";
import Game from "./pages/game/Game";

function AppContent() {
  return (
    <Routes>
      {/* ================= AUTH ================= */}

      <Route path="/" element={<Auth />} />

      <Route path="/login" element={<Auth />} />

      {/* ================= APP ================= */}

      <Route element={<DashboardLayout />}>
        <Route
          path="/dashboard"
          element={
            <ProtectedRoute>
              <Dashboard />
            </ProtectedRoute>
          }
        />

        <Route
          path="/marketplace"
          element={<ProtectedRoute><Marketplace /></ProtectedRoute>}
        />

        <Route
          path="/wallet"
          element={<Wallet />}
        />

        <Route
          path="/account"
          element={<ProtectedRoute><Account /></ProtectedRoute>}
        />

        <Route
          path="/create-nft"
          element={<ProtectedRoute><CreateNFT /></ProtectedRoute>}
        />

        <Route
          path="/game"
          element={
            <ProtectedRoute>
              <Game />
            </ProtectedRoute>
          }
        />

        <Route
          path="/nft/:id"
          element={<NFTDetails />}
        />
      </Route>
    </Routes>
  );
}

function App() {
  return (
    <BrowserRouter>
      <ThemeProvider>
        <WalletProvider>
          <div className="route-stage">
            <AppContent />
          </div>
        </WalletProvider>
      </ThemeProvider>
    </BrowserRouter>
  );
}

export default App;
