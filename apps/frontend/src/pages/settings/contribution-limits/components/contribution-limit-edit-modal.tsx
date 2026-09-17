import { Dialog, DialogContent } from "@wealthfolio/ui/components/ui/dialog";
import { ContributionLimitForm } from "./contribution-limit-form";
import type { ContributionLimit } from "@/lib/types";
import { useSettingsContext } from "@/lib/settings-provider";

interface ContributionLimitEditModalProps {
  limit: ContributionLimit | null;
  open: boolean;
  onClose: () => void;
}

export function ContributionLimitEditModal({
  limit,
  open,
  onClose,
}: ContributionLimitEditModalProps) {
  const { settings } = useSettingsContext();
  const isIndia = settings?.formattingRegion === "IN" || settings?.baseCurrency === "INR";
  const currentYear = new Date().getFullYear();

  const defaultStartDate = isIndia
    ? new Date(Date.UTC(currentYear, 3, 1, 12, 0, 0))
    : new Date(Date.UTC(currentYear, 0, 1, 12, 0, 0));
  const defaultEndDate = isIndia
    ? new Date(Date.UTC(currentYear + 1, 2, 31, 12, 0, 0))
    : new Date(Date.UTC(currentYear, 11, 31, 12, 0, 0));

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-[650px]">
        <ContributionLimitForm
          defaultValues={
            limit ?? {
              groupName: "",
              contributionYear: currentYear,
              accountIds: "",
              startDate: defaultStartDate,
              endDate: defaultEndDate,
            }
          }
          onSuccess={() => {
            onClose();
          }}
        />
      </DialogContent>
    </Dialog>
  );
}
