package com.roomwave.receiver
import org.junit.Assert.*
import org.junit.Test
class RepairTrackerTest {
 @Test fun stalledDeliveryCanBeRepairedBeforeDeadline() {
  val t=RepairTracker();t.observe(0,80_000_000,0)
  t.predict(6_000_000);assertTrue(t.missing.isEmpty())
  t.predict(18_000_000);val g=t.missing[240]!!
  assertEquals(85_000_000L,g.deadline)
  assertTrue(RepairTracker.canRepair(g.deadline,18_000_000,36.0,10.0))
  g.attempts++;g.lastRequest=7_000_000
  assertEquals(1,t.observe(240,65_000_000,17_000_000)!!.attempts)
  assertFalse(t.missing.containsKey(240))
 }
 @Test fun reorderWithinGraceNeedsNoRepair() {
  val t=RepairTracker();t.observe(0,80_000_000,0);t.predict(6_000_000)
  t.observe(240,65_000_000,6_000_000);assertTrue(t.missing.isEmpty())
 }
 @Test fun sequenceGapAndFecArrival() {
  val t=RepairTracker();t.observe(0,60_000_000,0);t.observe(480,70_000_000,10_000_000)
  assertEquals(65_000_000L,t.missing[240]!!.deadline)
  t.observe(240,65_000_000,11_000_000);assertTrue(t.missing.isEmpty())
 }
 @Test fun silenceIsBoundedAndNewTimelineResumes() {
  val t=RepairTracker();t.observe(0,80_000_000,0);t.predict(10_000_000_000)
  assertEquals(32,t.missing.size);t.missing.clear();t.predict(20_000_000_000)
  assertTrue(t.missing.isEmpty());t.observe(96000,20_060_000_000,20_000_000_000)
  t.predict(20_018_000_000);assertEquals(setOf(96240L),t.missing.keys)
 }
 @Test fun expiredDeadlinePreventsRequest() {
  assertFalse(RepairTracker.canRepair(60_000_000,11_000_000,36.0,10.0))
  assertTrue(RepairTracker.canRepair(60_000_000,9_000_000,36.0,10.0))
 }
 @Test fun tenAndTwelveMillisecondBurstsDoNotTriggerPrediction() {
  val t=RepairTracker()
  var time=0L;var frame=0L
  repeat(100) {
   t.observe(frame, time+70_000_000,time)
   t.observe(frame+240,time+75_000_000,time)
   for(ms in 2..11) t.predict(time+ms*1_000_000)
   assertEquals(0L,t.predicted)
   time+=12_000_000;frame+=480
  }
 }
 @Test fun repairArrivalDoesNotTeachBurstTiming() {
  val t=RepairTracker();t.observe(0,80_000_000,0)
  t.observe(240,85_000_000,17_000_000,false)
  assertEquals(18_000_000L,t.stallGraceNs)
 }
 @Test fun requestRateIsBoundedEvenDuringLongStall() {
  val t=RepairTracker()
  repeat(4) { assertTrue(t.allowRequest(0)) }
  assertFalse(t.allowRequest(0))
  assertFalse(t.allowRequest(50_000_000))
  assertTrue(t.allowRequest(100_000_000))
  assertFalse(t.allowRequest(100_000_000))
 }
}
