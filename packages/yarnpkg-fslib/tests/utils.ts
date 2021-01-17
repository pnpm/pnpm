export async function useFakeTime(cb: (advanceTimeBy: (ms: number) => void) => void | Promise<void>) {
  jest.useFakeTimers();

  let time = Date.now();
  jest.spyOn(Date, `now`).mockImplementation(() => time);

  const advanceTimeBy = (ms: number) => {
    time += ms;
    jest.advanceTimersByTime(ms);
  };

  await cb(advanceTimeBy);

  jest.restoreAllMocks();
}
