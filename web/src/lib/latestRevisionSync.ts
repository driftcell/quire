/**
 * Run one revision sync at a time while retaining the highest requested target.
 * A request arriving during an active sync joins the same drain loop, which
 * performs at most one trailing sync for the latest target.
 */
export class LatestRevisionSync {
  private targetRevision = 0;
  private forceRequested = false;
  private active: Promise<void> | null = null;
  private readonly readAppliedRevision: () => number;
  private readonly run: (fromRevision: number) => Promise<number>;

  constructor(
    readAppliedRevision: () => number,
    run: (fromRevision: number) => Promise<number>,
  ) {
    this.readAppliedRevision = readAppliedRevision;
    this.run = run;
  }

  request(revision: number, force = false): Promise<void> {
    this.targetRevision = Math.max(this.targetRevision, revision);
    this.forceRequested ||= force;
    if (this.active) {
      return this.active;
    }
    const active = this.drain().finally(() => {
      if (this.active === active) {
        this.active = null;
      }
    });
    this.active = active;
    return active;
  }

  private async drain(): Promise<void> {
    while (
      this.readAppliedRevision() < this.targetRevision ||
      this.forceRequested
    ) {
      const fromRevision = this.readAppliedRevision();
      this.forceRequested = false;
      const returnedRevision = await this.run(fromRevision);
      if (
        returnedRevision <= fromRevision &&
        this.readAppliedRevision() <= fromRevision &&
        this.targetRevision > fromRevision
      ) {
        throw new Error("Document sync made no revision progress");
      }
    }
  }
}
