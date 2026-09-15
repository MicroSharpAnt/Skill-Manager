import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Check, LoaderCircle, X } from "lucide-react";
import "./operation-toast.css";

export function OperationToast({ error = "", notice = "", busy = "", clearError, clearNotice }: {
  error?: string;
  notice?: string;
  busy?: string;
  clearError: () => void;
  clearNotice: () => void;
}) {
  const [showProgress, setShowProgress] = useState(false);
  const [paused, setPaused] = useState(false);
  const clearNoticeRef = useRef(clearNotice);
  clearNoticeRef.current = clearNotice;
  useEffect(() => setPaused(false), [notice, busy, error]);
  useEffect(() => {
    setShowProgress(false);
    if (!busy) return;
    const timer = window.setTimeout(() => setShowProgress(true), 200);
    return () => window.clearTimeout(timer);
  }, [busy]);
  useEffect(() => {
    if (!notice || busy || error || paused) return;
    const timer = window.setTimeout(() => clearNoticeRef.current(), 5000);
    return () => window.clearTimeout(timer);
  }, [notice, busy, error, paused]);

  const kind = error ? "error" : busy ? "progress" : "success";
  const message = error || (busy ? (showProgress ? `${busy}…` : "") : notice);
  if (!message) return null;
  return <div className={`operation-toast operation-toast-${kind}`}
    onMouseEnter={() => setPaused(true)} onMouseLeave={() => setPaused(false)}
    onFocus={() => setPaused(true)} onBlur={event => {
      if (!event.currentTarget.contains(event.relatedTarget)) setPaused(false);
    }}>
    <div className="operation-toast-message" role={error ? "alert" : "status"} aria-atomic="true">
      {error ? <AlertTriangle size={17} /> : busy ? <LoaderCircle size={17} className="spin" /> : <Check size={17} />}
      <span>{message}</span>
    </div>
    {!busy && <button type="button" aria-label={error ? "关闭错误提示" : "关闭操作提示"}
      onClick={error ? clearError : clearNotice}><X size={15} /></button>}
  </div>;
}
