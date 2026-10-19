import { useLanguage } from "../i18n";
import "./LanguageButton.css";

export default function LanguageButton() {
  const { lang, toggleLang } = useLanguage();

  return (
    <button type="button" className="language-button" onClick={toggleLang}>
      {lang.toUpperCase()}
    </button>
  );
}
