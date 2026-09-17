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
    ? new Date(currentYear, 3, 1)
    : new Date(currentYear, 0, 1);
  const defaultEndDate = isIndia
    ? new Date(currentYear + 1, 2, 31)
    : new Date(currentYear, 11, 31);

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="overflow-y-auto [--overlay-max-height:90vh] sm:max-w-162.5">
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
