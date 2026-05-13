import React, { useState, useEffect, useCallback, createContext, useContext } from 'react';
import { X, CheckCircle, AlertTriangle, Info, AlertOctagon } from 'lucide-react';

export type ToastType = 'success' | 'error' | 'warning' | 'info';

export interface ToastMessage {
  id: string;
  type: ToastType;
  title?: string;
  message: string;
  duration?: number;
}

interface ToastContextType {
  showToast: (type: ToastType, message: string, title?: string, duration?: number) => void;
  removeToast: (id: string) => void;
}

const ToastContext = createContext<ToastContextType | undefined>(undefined);

export const useToast = () => {
  const context = useContext(ToastContext);
  if (!context) {
    throw new Error('useToast must be used within a ToastProvider');
  }
  return context;
};

export const ToastProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const showToast = useCallback(
    (type: ToastType, message: string, title?: string, duration = 3000) => {
      const id = Math.random().toString(36).substring(2, 9);
      const newToast: ToastMessage = { id, type, title, message, duration };
      setToasts((prev) => [...prev, newToast]);
      // Auto-dismiss is handled by ToastItem's useEffect — no duplicate timer here
    },
    []
  );

  return (
    <ToastContext.Provider value={{ showToast, removeToast }}>
      {children}
      <div className="fixed top-4 right-4 z-[9999] flex flex-col gap-2 w-full max-w-sm pointer-events-none">
        {toasts.map((toast) => (
          <ToastItem key={toast.id} toast={toast} onRemove={removeToast} />
        ))}
      </div>
    </ToastContext.Provider>
  );
};

const ToastItem: React.FC<{ toast: ToastMessage; onRemove: (id: string) => void }> = ({
  toast,
  onRemove,
}) => {
  const [isExiting, setIsExiting] = useState(false);

  const handleRemove = useCallback(() => {
    setIsExiting(true);
    setTimeout(() => onRemove(toast.id), 300); // Wait for animation
  }, [onRemove, toast.id]);

  useEffect(() => {
    if (toast.duration && toast.duration > 0) {
      const timer = setTimeout(() => {
        handleRemove();
      }, toast.duration);
      return () => clearTimeout(timer);
    }
  }, [toast.duration, handleRemove]);

  const getIcon = () => {
    switch (toast.type) {
      case 'success':
        return <CheckCircle className="w-5 h-5 text-cyber-text" />;
      case 'error':
        return <AlertOctagon className="w-5 h-5 text-cyber-error" />;
      case 'warning':
        return <AlertTriangle className="w-5 h-5 text-cyber-warning" />;
      case 'info':
        return <Info className="w-5 h-5 text-cyber-text-secondary" />;
    }
  };

  const getBorderColor = () => {
    switch (toast.type) {
      case 'success':
        return 'border-cyber-border/50';
      case 'error':
        return 'border-cyber-error/50';
      case 'warning':
        return 'border-cyber-warning/50';
      case 'info':
        return 'border-cyber-border';
    }
  };

  return (
    <div
      className={`
                pointer-events-auto
                relative overflow-hidden
                bg-cyber-surface/95 backdrop-blur-md
                border-l-4 ${getBorderColor()}
                text-cyber-text
                p-4 shadow-lg rounded-lg
                transition-all duration-300 ease-in-out
                ${isExiting ? 'translate-x-[120%] opacity-0' : 'translate-x-0 opacity-100'}
                animate-in slide-in-from-right-1/2 fade-in
                group
                flex items-start gap-3
            `}
      role="alert"
    >
      <div className="flex-shrink-0 mt-0.5">{getIcon()}</div>
      <div className="flex-1 min-w-0">
        {toast.title && <h4 className="text-sm font-semibold mb-1">{toast.title}</h4>}
        <p className="text-sm text-cyber-text-secondary">{toast.message}</p>
      </div>
      <button
        onClick={handleRemove}
        className="flex-shrink-0 text-cyber-text-secondary hover:text-cyber-text transition-colors"
        aria-label="Close"
      >
        <X className="w-4 h-4" />
      </button>

      {/* Progress bar for auto-dismiss */}
      {toast.duration && toast.duration > 0 && (
        <div
          className={`absolute bottom-0 left-0 h-0.5 bg-current opacity-30 origin-left animate-progress ${
            toast.type === 'success'
              ? 'text-cyber-text'
              : toast.type === 'error'
                ? 'text-cyber-error'
                : 'text-cyber-text-secondary'
          }`}
          style={{ width: '100%', animationDuration: `${toast.duration}ms` }}
        />
      )}
    </div>
  );
};
