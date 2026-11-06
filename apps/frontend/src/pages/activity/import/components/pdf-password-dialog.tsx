import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

export interface PdfPasswordDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (password: string) => void;
  isLoading?: boolean;
  error?: string | null;
}

export function PdfPasswordDialog({
  open,
  onOpenChange,
  onSubmit,
  isLoading = false,
  error = null,
}: PdfPasswordDialogProps) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);

  useEffect(() => {
    if (open) {
      setPassword("");
      setShowPassword(false);
    }
  }, [open]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (password.trim()) {
      onSubmit(password);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <form onSubmit={handleSubmit} className="space-y-4">
          <DialogHeader>
            <div className="flex items-center gap-2">
              <div className="flex h-9 w-9 items-center justify-center rounded-full bg-amber-100 dark:bg-amber-900/30">
                <Icons.Lock className="h-4 w-4 text-amber-600 dark:text-amber-400" />
              </div>
              <DialogTitle>{t("activity:import.upload.pdfPasswordTitle")}</DialogTitle>
            </div>
            <DialogDescription>
              {t("activity:import.upload.pdfPasswordDescription")}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2">
            <Label htmlFor="pdf-password" className="text-xs">
              {t("activity:import.upload.pdfPasswordPlaceholder")}
            </Label>
            <div className="relative">
              <Input
                id="pdf-password"
                type={showPassword ? "text" : "password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••••••••"
                className="pr-10"
                autoFocus
                disabled={isLoading}
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="text-muted-foreground hover:text-foreground absolute right-2 top-1/2 -translate-y-1/2 p-1 text-xs"
              >
                {showPassword ? (
                  <Icons.EyeOff className="h-4 w-4" />
                ) : (
                  <Icons.Eye className="h-4 w-4" />
                )}
              </button>
            </div>

            {error && <p className="text-destructive text-xs">{error}</p>}

            <p className="text-muted-foreground bg-muted/30 rounded-md p-2 text-xs leading-relaxed">
              <span className="font-medium text-foreground">💡 Hint: </span>
              {t("activity:import.upload.pdfPasswordHint")}
            </p>
          </div>

          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={isLoading}
            >
              {t("activity:import.toolbar.cancel")}
            </Button>
            <Button type="submit" disabled={!password.trim() || isLoading}>
              {isLoading && <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />}
              {t("activity:import.upload.pdfPasswordSubmit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default PdfPasswordDialog;
