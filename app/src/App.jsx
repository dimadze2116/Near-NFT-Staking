import StakeScreen from "./components/StakeScreen";
import { LanguageProvider } from "./i18n";
import { WalletProvider } from "./WalletContext";
import { ThemeProvider } from "./ThemeContext";
import "./theme.css";
import "./App.css";

export default function App() {
  return (
    <ThemeProvider>
      <LanguageProvider>
        <WalletProvider>
          <div className="app-stage">
            <div className="app-stage__frame">
              <StakeScreen />
            </div>
          </div>
        </WalletProvider>
      </LanguageProvider>
    </ThemeProvider>
  );
}
