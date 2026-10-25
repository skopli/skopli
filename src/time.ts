function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const m = month;
  const doy = Math.floor((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146_097 + doe - 719_468;
}

// YYYY-MM-DD (T HH:mm(:ss(.sss)?)?)? (Z | ±HH:mm)? to epoch millis; offsetless is UTC,
// anything else is undefined.
export function dateParseMs(input: string): number | undefined {
  const isDigits = (from: number, len: number): number | undefined => {
    const slice = input.slice(from, from + len);
    if (slice.length !== len || !/^[0-9]+$/.test(slice)) return undefined;
    return Number(slice);
  };
  if (input.length < 10 || input[4] !== "-" || input[7] !== "-") return undefined;
  const year = isDigits(0, 4);
  const month = isDigits(5, 2);
  const day = isDigits(8, 2);
  if (year === undefined || month === undefined || day === undefined) return undefined;
  if (month < 1 || month > 12 || day < 1 || day > 31) return undefined;
  let hour = 0;
  let minute = 0;
  let second = 0;
  let milli = 0;
  let offsetMin = 0;
  if (input.length > 10) {
    if (input[10] !== "T" && input[10] !== " ") return undefined;
    if (input.length < 16 || input[13] !== ":") return undefined;
    const h = isDigits(11, 2);
    const mi = isDigits(14, 2);
    if (h === undefined || mi === undefined) return undefined;
    hour = h;
    minute = mi;
    let idx = 16;
    if (input.length > idx && input[idx] === ":") {
      const sec = isDigits(17, 2);
      if (sec === undefined) return undefined;
      second = sec;
      idx = 19;
      if (input.length > idx && input[idx] === ".") {
        const fracStart = idx + 1;
        let fracEnd = fracStart;
        while (fracEnd < input.length && /[0-9]/.test(input[fracEnd] ?? "")) fracEnd += 1;
        if (fracEnd === fracStart) return undefined;
        milli = Number(input.slice(fracStart, fracEnd).padEnd(3, "0").slice(0, 3));
        idx = fracEnd;
      }
    }
    if (input.length > idx) {
      const c = input[idx];
      if (c === "Z") {
        if (input.length !== idx + 1) return undefined;
      } else if (c === "+" || c === "-") {
        const sign = c === "-" ? -1 : 1;
        if (input.length !== idx + 6 || input[idx + 3] !== ":") return undefined;
        const oh = isDigits(idx + 1, 2);
        const om = isDigits(idx + 4, 2);
        if (oh === undefined || om === undefined) return undefined;
        offsetMin = sign * (oh * 60 + om);
      } else {
        return undefined;
      }
    }
  }
  if (hour < 0 || hour > 23) return undefined;
  if (minute < 0 || minute > 59) return undefined;
  if (second < 0 || second > 59) return undefined;
  const days = daysFromCivil(year, month, day);
  return (
    days * 86_400_000 +
    hour * 3_600_000 +
    minute * 60_000 +
    second * 1_000 +
    milli -
    offsetMin * 60_000
  );
}
