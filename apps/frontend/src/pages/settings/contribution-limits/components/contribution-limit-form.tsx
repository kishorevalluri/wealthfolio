import { zodResolver } from "@hookform/resolvers/zod";
import { useForm, useWatch } from "react-hook-form";
import { useTranslation } from "react-i18next";
import * as z from "zod";
import { useEffect } from "react";

import { Button } from "@wealthfolio/ui/components/ui/button";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Card, CardContent } from "@wealthfolio/ui/components/ui/card";

import {
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
  FormDescription,
  DatePickerInput,
  Input,
} from "@wealthfolio/ui";

import { newContributionLimitSchema } from "@/lib/schemas";
import { useContributionLimitMutations } from "../use-contribution-limit-mutations";
import { MoneyInput } from "@wealthfolio/ui";

type NewContributionLimit = z.infer<typeof newContributionLimitSchema>;

type ContributionLimitFormValues = Omit<NewContributionLimit, "limitAmount"> & {
  limitAmount?: number;
};

interface ContributionLimitFormProps {
  defaultValues?: ContributionLimitFormValues;
  onSuccess?: () => void;
}

interface LimitPreset {
  name: string;
  label: string;
  amount: number;
  isFinancialYear?: boolean;
  category?: "in" | "us" | "ca";
}

const LIMIT_PRESETS: LimitPreset[] = [
  // India (Income Tax Act & CG Exemptions)
  { name: "Section 80C (PPF/ELSS/EPF)", label: "80C (₹1.5L)", amount: 150000, isFinancialYear: true, category: "in" },
  { name: "Section 80CCD(1B) NPS", label: "80CCD(1B) NPS (₹50k)", amount: 50000, isFinancialYear: true, category: "in" },
  { name: "Section 80D Self & Family Health", label: "80D Health (₹25k)", amount: 25000, isFinancialYear: true, category: "in" },
  { name: "Section 80D Senior Citizen Parents", label: "80D Parents (₹50k)", amount: 50000, isFinancialYear: true, category: "in" },
  { name: "Section 80CCD(2) Employer NPS", label: "80CCD(2) Corp NPS", amount: 100000, isFinancialYear: true, category: "in" },
  { name: "Section 24(b) Home Loan Interest", label: "24(b) Home Loan (₹2L)", amount: 200000, isFinancialYear: true, category: "in" },
  { name: "Section 80TTA Savings Interest", label: "80TTA Interest (₹10k)", amount: 10000, isFinancialYear: true, category: "in" },
  { name: "Section 80TTB Senior Interest", label: "80TTB Sr Interest (₹50k)", amount: 50000, isFinancialYear: true, category: "in" },
  { name: "Section 80E Education Loan", label: "80E Edu Loan", amount: 50000, isFinancialYear: true, category: "in" },
  { name: "Section 80G Donations", label: "80G Donations", amount: 25000, isFinancialYear: true, category: "in" },
  { name: "Section 112A LTCG Equity Exemption", label: "112A LTCG Exemption (₹1.25L)", amount: 125000, isFinancialYear: true, category: "in" },
  { name: "Section 54EC Capital Gains Bonds", label: "54EC CG Bonds (₹50L)", amount: 5000000, isFinancialYear: true, category: "in" },

  // US & Others
  { name: "Traditional / Roth IRA", label: "IRA ($7k)", amount: 7000, category: "us" },
  { name: "401(k)", label: "401(k) ($23k)", amount: 23000, category: "us" },
  { name: "HSA Individual", label: "HSA ($4.15k)", amount: 4150, category: "us" },
  { name: "TFSA", label: "TFSA ($7k)", amount: 7000, category: "ca" },
  { name: "RRSP", label: "RRSP ($31.5k)", amount: 31560, category: "ca" },
];

import { useSettingsContext } from "@/lib/settings-provider";

export function ContributionLimitForm({
  defaultValues,
  onSuccess = () => {},
}: ContributionLimitFormProps) {
  const { t } = useTranslation();
  const { settings } = useSettingsContext();
  const isIndia = settings?.formattingRegion === "IN" || settings?.baseCurrency === "INR";
  const { addContributionLimitMutation, updateContributionLimitMutation } =
    useContributionLimitMutations();

  const form = useForm<NewContributionLimit>({
    resolver: zodResolver(newContributionLimitSchema),
    defaultValues: {
      ...defaultValues,
      startDate: defaultValues?.startDate ? new Date(defaultValues.startDate) : undefined,
      endDate: defaultValues?.endDate ? new Date(defaultValues.endDate) : undefined,
    },
  });

  const sortedPresets = [
    ...LIMIT_PRESETS.filter((p) => (isIndia ? p.category === "in" : p.category !== "in")),
    ...LIMIT_PRESETS.filter((p) => (isIndia ? p.category !== "in" : p.category === "in")),
  ];

  const currentStartDate = useWatch({
    control: form.control,
    name: "startDate",
  });
  const isCurrentlyFinancialYear =
    currentStartDate instanceof Date && currentStartDate.getUTCMonth() === 3;

  // Function to update dates based on year
  const updateDatesBasedOnYear = (year: number) => {
    if (!year || isNaN(year)) return;
    const currentStart = form.getValues("startDate");
    const isFY = currentStart instanceof Date && currentStart.getUTCMonth() === 3;

    if (isFY) {
      const startDate = new Date(Date.UTC(year, 3, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(year + 1, 2, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    } else {
      const startDate = new Date(Date.UTC(year, 0, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(year, 11, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    }
  };

  const setPeriodType = (type: "calendar" | "financial") => {
    const yr = form.getValues("contributionYear") || new Date().getFullYear();
    if (type === "financial") {
      const startDate = new Date(Date.UTC(yr, 3, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(yr + 1, 2, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    } else {
      const startDate = new Date(Date.UTC(yr, 0, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(yr, 11, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    }
  };

  const applyPreset = (preset: LimitPreset) => {
    form.setValue("groupName", preset.name);
    form.setValue("limitAmount", preset.amount);
    const yr = form.getValues("contributionYear") || new Date().getFullYear();
    if (preset.isFinancialYear) {
      const startDate = new Date(Date.UTC(yr, 3, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(yr + 1, 2, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    } else {
      const startDate = new Date(Date.UTC(yr, 0, 1, 12, 0, 0));
      const endDate = new Date(Date.UTC(yr, 11, 31, 12, 0, 0));
      form.setValue("startDate", startDate);
      form.setValue("endDate", endDate);
    }
  };

  // Watch for changes to contributionYear
  const contributionYear = useWatch({
    control: form.control,
    name: "contributionYear",
  });

  // Update dates when year changes
  useEffect(() => {
    if (contributionYear) {
      updateDatesBasedOnYear(contributionYear);
    }
  }, [contributionYear]);

  function onSubmit(data: NewContributionLimit) {
    const { id, ...rest } = data;

    // Convert date objects to ISO strings (RFC 3339 compatible format)
    const formattedData = {
      ...rest,
      startDate: rest.startDate instanceof Date ? rest.startDate.toISOString() : rest.startDate,
      endDate: rest.endDate instanceof Date ? rest.endDate.toISOString() : rest.endDate,
    };

    if (id) {
      return updateContributionLimitMutation.mutate(
        { id, updatedLimit: formattedData },
        { onSuccess },
      );
    }
    return addContributionLimitMutation.mutate(formattedData, { onSuccess });
  }

  return (
    <Form {...form}>
      <form onSubmit={form.handleSubmit(onSubmit)} className="mx-auto w-full max-w-4xl space-y-8">
        <DialogHeader className="px-1">
          <DialogTitle className="text-2xl font-semibold">
            {defaultValues?.id
              ? t("settings:limits_form_update_title")
              : t("settings:limits_form_add_title")}
          </DialogTitle>
          <DialogDescription className="text-muted-foreground mt-1 text-base">
            {defaultValues?.id
              ? t("settings:limits_form_update_description")
              : t("settings:limits_form_add_description")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-6 px-1">
          {/* Hidden id field */}
          <input type="hidden" name="id" />

          {/* Form content */}
          <Card className="border-border/40 w-full overflow-hidden rounded-lg border shadow-sm">
            <CardContent className="p-6">
              <div className="space-y-6">
                <div className="space-y-2">
                  <div className="text-muted-foreground flex items-center justify-between text-xs font-medium">
                    <span>{t("settings:limits_form_presets_label", { defaultValue: "Presets" })}</span>
                    <div className="flex gap-1">
                      <Button
                        type="button"
                        size="sm"
                        variant={isCurrentlyFinancialYear ? "secondary" : "ghost"}
                        className="h-6 px-2 text-xs"
                        onClick={() => setPeriodType("financial")}
                      >
                        FY (Apr–Mar)
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant={!isCurrentlyFinancialYear ? "secondary" : "ghost"}
                        className="h-6 px-2 text-xs"
                        onClick={() => setPeriodType("calendar")}
                      >
                        CY (Jan–Dec)
                      </Button>
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {sortedPresets.map((preset) => (
                      <Button
                        key={preset.name}
                        type="button"
                        size="sm"
                        variant="outline"
                        className="h-7 text-xs font-normal"
                        onClick={() => applyPreset(preset)}
                      >
                        {preset.label}
                      </Button>
                    ))}
                  </div>
                </div>

                <FormField
                  control={form.control}
                  name="groupName"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="text-base font-medium">
                        {t("settings:limits_form_group_name_label")}
                      </FormLabel>
                      <FormControl>
                        <Input
                          placeholder={t("settings:limits_form_group_name_placeholder")}
                          {...field}
                          className="h-11 text-base"
                        />
                      </FormControl>
                      <FormDescription className="text-muted-foreground mt-1 text-sm">
                        {t("settings:limits_form_group_name_description")}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="limitAmount"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="text-base font-medium">
                        {t("settings:limits_form_limit_amount_label")}
                      </FormLabel>
                      <FormControl>
                        <MoneyInput
                          placeholder={t("settings:limits_form_limit_amount_placeholder")}
                          {...field}
                        />
                      </FormControl>
                      <FormDescription className="text-muted-foreground mt-1 text-sm">
                        {t("settings:limits_form_limit_amount_description")}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <div className="space-y-2">
                  <div className="grid grid-cols-1 gap-5 md:grid-cols-3">
                    <FormField
                      control={form.control}
                      name="contributionYear"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel className="text-base font-medium">
                            {t("settings:limits_form_year_label")}
                          </FormLabel>
                          <FormControl>
                            <Input
                              type="number"
                              placeholder={t("settings:limits_form_year_placeholder")}
                              value={field.value || ""}
                              onChange={(e) => {
                                const numValue =
                                  e.target.value === "" ? undefined : Number(e.target.value);
                                field.onChange(numValue);
                              }}
                            />
                          </FormControl>

                          <FormMessage />
                        </FormItem>
                      )}
                    />

                    <FormField
                      control={form.control}
                      name="startDate"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel className="text-base font-medium">
                            {t("settings:limits_form_start_date_label")}
                          </FormLabel>
                          <div className="h-11">
                            <FormControl>
                              <DatePickerInput
                                onChange={(date: Date | undefined) => field.onChange(date)}
                                value={field.value as Date | undefined}
                                disabled={field.disabled}
                              />
                            </FormControl>
                          </div>
                          <FormMessage />
                        </FormItem>
                      )}
                    />

                    <FormField
                      control={form.control}
                      name="endDate"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel className="text-base font-medium">
                            {t("settings:limits_form_end_date_label")}
                          </FormLabel>
                          <div className="h-11">
                            <FormControl>
                              <DatePickerInput
                                onChange={(date: Date | undefined) => field.onChange(date)}
                                value={field.value as Date | undefined}
                                disabled={field.disabled}
                              />
                            </FormControl>
                          </div>
                          <FormMessage />
                        </FormItem>
                      )}
                    />
                  </div>

                  <p className="text-muted-foreground mt-2 text-sm italic">
                    {t("settings:limits_form_date_description")}
                  </p>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>

        <DialogFooter className="px-1 pt-2">
          <div className="flex w-full justify-end gap-3">
            <DialogTrigger asChild>
              <Button variant="outline" type="button" className="h-11 min-w-24 text-base">
                {t("common:cancel")}
              </Button>
            </DialogTrigger>
            <Button
              type="submit"
              disabled={
                addContributionLimitMutation.isPending || updateContributionLimitMutation.isPending
              }
              className="h-11 min-w-24 text-base"
            >
              {addContributionLimitMutation.isPending ||
              updateContributionLimitMutation.isPending ? (
                <>
                  <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />
                  {defaultValues?.id
                    ? t("settings:limits_form_updating")
                    : t("settings:limits_form_saving")}
                </>
              ) : (
                <>
                  <Icons.Check className="mr-2 h-4 w-4" />
                  {defaultValues?.id
                    ? t("settings:limits_form_update_button")
                    : t("settings:limits_form_save_button")}
                </>
              )}
            </Button>
          </div>
        </DialogFooter>
      </form>
    </Form>
  );
}
