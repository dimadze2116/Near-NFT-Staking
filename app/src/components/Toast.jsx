import { useEffect } from "react";
import "./Toast.css";

// Всплывающая плашка по центру экрана. Появление — как у кнопки стейка
// (toast-appear), уход — плавное затухание. Автозакрытие через TOAST_MS.
const TOAST_MS = 2600;

function SuccessIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M5 12.5l4.2 4.2L19 7"
        stroke="currentColor"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ErrorIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M7 7l10 10M17 7L7 17"
        stroke="currentColor"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export default function Toast({ variant = "success", title, text, exiting = false, onClose }) {
  useEffect(() => {
    if (exiting) return;
    const id = setTimeout(onClose, TOAST_MS);
    return () => clearTimeout(id);
  }, [exiting, onClose]);

  return (
    <div className="toast-overlay" aria-hidden="true">
      <div
        className={`toast toast--${variant}${exiting ? " toast--exiting" : ""}`}
        role="status"
        aria-live="polite"
      >
        {/* Перелив и окантовка как у подложки периода */}
        <span className="toast__highlight" aria-hidden="true" />
        <span className={`toast__icon toast__icon--${variant}`}>
          {variant === "success" ? <SuccessIcon /> : <ErrorIcon />}
        </span>
        <div className="toast__body">
          <span className="toast__title">{title}</span>
          {text && <span className="toast__text">{text}</span>}
        </div>
      </div>
    </div>
  );
}
