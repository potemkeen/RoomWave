#include <aaudio/AAudio.h>
#include <jni.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <ctime>

namespace {
constexpr int frames = 240, capacity = 256;
static_assert(std::atomic<double>::is_always_lock_free, "Audio metrics must not lock");
static_assert(std::atomic<int64_t>::is_always_lock_free, "Audio timestamps must not lock");
int64_t nowNs() { timespec t{}; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1000000000LL + t.tv_nsec; }
struct Packet {
    int64_t frame=0, capture=0, send=0, play=0, received=0;
    std::array<int16_t, frames*2> pcm{};
};
// Exactly one JNI network producer and one audio consumer. Slot ownership protects
// every non-atomic payload access. Neither side waits for the other.
struct Slot { std::atomic<int> state{0}; Packet packet; };
struct Engine {
    AAudioStream* stream=nullptr;
    std::array<Slot,capacity> slots;
    Slot recovery;
    int64_t lastRecoveryFrame=-1;
    int recoveryRun=0;
    Packet pendingRecovery;
    bool recovering=false;
    std::atomic<int64_t> offset{0}, clockAt{0}, minimumFrame{-1};
    std::atomic<int64_t> stampFrame{0}, stampNs{0}, stampAt{0};
    std::atomic<uint32_t> stampVersion{0};
    std::atomic<int64_t> late{0}, loss{0}, accepted{0}, rendered{0};
    std::atomic<int> error{0}, status{0};
    std::atomic<double> phase{0}, queueMs{0}, outputMs{0}, latencyMs{-1}, networkMs{0}, captureMs{0};
    int64_t expected=-1, anchorFrame=0, anchorPlay=0;
    Packet current;
    bool have=false, loaded=false, measured=false;
    double cursor=0, gain=0;
    float lastL=0,lastR=0;
    int lastXruns=0;
    int64_t stableAt=0;

    bool take(int64_t frame, Packet& out) {
        auto& s=slots[(frame/frames)%capacity];
        int ready=2;
        if(!s.state.compare_exchange_strong(ready,3,std::memory_order_acquire)) return false;
        if(s.packet.frame!=frame) { s.state.store(2,std::memory_order_release); return false; }
        out=s.packet;
        s.state.store(0,std::memory_order_release);
        return true;
    }
    bool push(const Packet& p) {
        const auto floor=minimumFrame.load();
        if(floor>=0 && p.frame<floor) {
            late.fetch_add(1);
            const auto now=nowNs();
            // A run of increasing source frames with FUTURE host deadlines proves
            // a timeline discontinuity, rather than ordinary network reordering.
            const auto remaining=p.play+offset.load()-now;
            if(now-clockAt.load()<2000000000LL && remaining>(outputMs.load()+5)*1e6 && remaining<1000000000LL) {
                recoveryRun=p.frame==lastRecoveryFrame+frames ? recoveryRun+1 : 1;
                lastRecoveryFrame=p.frame;
                if(recoveryRun>=3) {
                    int free=0;
                    if(recovery.state.compare_exchange_strong(free,1,std::memory_order_acquire)) {
                        recovery.packet=p; recovery.state.store(2,std::memory_order_release);
                        recoveryRun=0;
                    }
                }
            } else recoveryRun=0;
            return false;
        }
        recoveryRun=0;
        auto& s=slots[(p.frame/frames)%capacity];
        int free=0;
        if(!s.state.compare_exchange_strong(free,1,std::memory_order_acquire)) {
            int ready=2;
            if(!s.state.compare_exchange_strong(ready,1,std::memory_order_acquire)) return false;
            if(s.packet.frame>=floor) { s.state.store(2,std::memory_order_release); return false; }
        }
        s.packet=p; s.state.store(2,std::memory_order_release);
        accepted.fetch_add(1); return true;
    }
    bool timestamp(int64_t& frame,int64_t& ns) {
        auto v=stampVersion.load(std::memory_order_acquire);
        if(v&1) return false;
        frame=stampFrame.load(std::memory_order_relaxed); ns=stampNs.load(std::memory_order_relaxed);
        return stampVersion.load(std::memory_order_acquire)==v && ns>0;
    }
    void render(float* out,int count) {
        const auto now=nowNs();
        const auto written=AAudioStream_getFramesWritten(stream);
        renderAt(out,count,now,written);
    }
    void renderAt(float* out,int count,int64_t now,int64_t written) {
        std::fill_n(out,count*2,0.f);
        int64_t sf=0,sn=0;
        rendered.store(written+count);
        if(now-clockAt.load()>2000000000LL || now-stampAt.load()>500000000LL || !timestamp(sf,sn)) {
            status.store(0); gain=0; return;
        }
        const auto localOffset=offset.load();
        if(!recovering) {
            int ready=2;
            if(recovery.state.compare_exchange_strong(ready,3,std::memory_order_acquire)) {
                pendingRecovery=recovery.packet; recovering=true;
                recovery.state.store(0,std::memory_order_release);
            }
        }
        if(expected<0) {
            int64_t first=INT64_MAX;
            for(auto& s:slots) {
                int ready=2;
                if(s.state.compare_exchange_strong(ready,3,std::memory_order_acquire)) {
                    first=std::min(first,s.packet.frame); s.state.store(2,std::memory_order_release);
                }
            }
            if(first==INT64_MAX) { status.store(1); return; }
            expected=first;
        }
        // All deadlines originate at the host. Arrival time is used only for metrics.
        for(int i=0;i<count;++i) {
            const int64_t presentation=sn+(written+i-sf)*1000000000LL/48000;
            if(recovering) {
                gain=std::max(0.,gain-1./240);
                out[2*i]=lastL*gain; out[2*i+1]=lastR*gain;
                if(gain==0) {
                    current=pendingRecovery; expected=current.frame;
                    anchorFrame=current.frame; anchorPlay=current.play;
                    minimumFrame.store(expected); cursor=0; loaded=true;
                    have=true; measured=false; recovering=false;
                }
                status.store(2); continue;
            }
            if(!loaded) {
                have=take(expected,current); loaded=true; measured=false;
                if(have) { anchorFrame=current.frame; anchorPlay=current.play; }
                if(anchorPlay==0) { loaded=false; status.store(1); continue; }
            }
            const auto target=(have?current.play:anchorPlay+(expected-anchorFrame)*1000000000LL/48000)+localOffset;
            const double err=double(presentation-target)-cursor*1000000000.0/48000;
            phase.store(err/1e6);
            if(err < -1000000 && cursor==0 && gain==0) { status.store(2); continue; }
            if(err>20000000) {
                // Fade out before an exceptional re-alignment; startup can skip silently.
                gain=std::max(0.,gain-1./240);
                out[2*i]=lastL*gain; out[2*i+1]=lastR*gain;
                if(gain==0) { expected+=frames; minimumFrame.store(expected); loaded=false; cursor=0; }
                status.store(2); continue;
            }
            if(!measured) {
                measured=true;
                if(!have) loss.fetch_add(1);
                else {
                    queueMs.store((now-current.received)/1e6);
                    outputMs.store((presentation-now)/1e6);
                    latencyMs.store(current.capture>0?(presentation-current.capture-localOffset)/1e6:-1);
                    networkMs.store((current.received-current.send-localOffset)/1e6);
                    captureMs.store((current.send-current.capture)/1e6);
                }
            }
            if(have) {
                const int a=std::min(int(cursor),frames-1), b=std::min(a+1,frames-1);
                const float fraction=float(cursor-a);
                lastL=(current.pcm[2*a]+fraction*(current.pcm[2*b]-current.pcm[2*a]))/32768.f;
                lastR=(current.pcm[2*a+1]+fraction*(current.pcm[2*b+1]-current.pcm[2*a+1]))/32768.f;
                gain=std::min(1.,gain+1./240);
            } else gain=std::max(0.,gain-1./240);
            out[2*i]=lastL*gain; out[2*i+1]=lastR*gain;
            // A bounded rate correction follows clock drift and coordinated target changes.
            cursor+=1.+std::clamp(err/1e9,-0.005,0.005);
            if(cursor>=frames) { cursor-=frames; expected+=frames; minimumFrame.store(expected); loaded=false; }
            status.store(std::abs(err)<3000000?3:2);
        }
    }
    void poll() {
        int64_t frame=0,ns=0;
        auto now=nowNs();
        if(AAudioStream_getTimestamp(stream,CLOCK_MONOTONIC,&frame,&ns)==AAUDIO_OK && std::abs(ns-now)<1000000000LL) {
            stampVersion.fetch_add(1,std::memory_order_acq_rel);
            stampFrame.store(frame,std::memory_order_relaxed); stampNs.store(ns,std::memory_order_relaxed);
            stampVersion.fetch_add(1,std::memory_order_release); stampAt.store(now);
        }
        const int xruns=AAudioStream_getXRunCount(stream), burst=AAudioStream_getFramesPerBurst(stream);
        const int size=AAudioStream_getBufferSizeInFrames(stream);
        if(xruns>lastXruns) {
            AAudioStream_setBufferSizeInFrames(stream,std::min(size+burst,burst*6)); stableAt=now;
        } else if(now-stableAt>30000000000LL && size>burst*2) {
            AAudioStream_setBufferSizeInFrames(stream,size-burst); stableAt=now;
        }
        lastXruns=xruns;
    }
};
aaudio_data_callback_result_t callback(AAudioStream*,void* user,void* data,int32_t n) {
    static_cast<Engine*>(user)->render(static_cast<float*>(data),n); return AAUDIO_CALLBACK_RESULT_CONTINUE;
}
void failed(AAudioStream*,void* user,aaudio_result_t error) { static_cast<Engine*>(user)->error.store(error); }
Engine* engine(jlong handle) { return reinterpret_cast<Engine*>(handle); }
}
extern "C" JNIEXPORT jlong JNICALL Java_com_roomwave_receiver_NativeAudio_open(JNIEnv*,jobject) {
    auto* e=new Engine;
    AAudioStreamBuilder* builder=nullptr;
    if(AAudio_createStreamBuilder(&builder)!=AAUDIO_OK) { delete e; return 0; }
    AAudioStreamBuilder_setDirection(builder,AAUDIO_DIRECTION_OUTPUT);
    AAudioStreamBuilder_setSampleRate(builder,48000);
    AAudioStreamBuilder_setChannelCount(builder,2);
    AAudioStreamBuilder_setFormat(builder,AAUDIO_FORMAT_PCM_FLOAT);
    AAudioStreamBuilder_setPerformanceMode(builder,AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
    AAudioStreamBuilder_setSharingMode(builder,AAUDIO_SHARING_MODE_EXCLUSIVE);
    AAudioStreamBuilder_setDataCallback(builder,callback,e);
    AAudioStreamBuilder_setErrorCallback(builder,failed,e);
    auto result=AAudioStreamBuilder_openStream(builder,&e->stream);
    if(result!=AAUDIO_OK) {
        AAudioStreamBuilder_setSharingMode(builder,AAUDIO_SHARING_MODE_SHARED);
        result=AAudioStreamBuilder_openStream(builder,&e->stream);
    }
    AAudioStreamBuilder_delete(builder);
    if(result!=AAUDIO_OK) { delete e; return 0; }
    if(AAudioStream_getSampleRate(e->stream)!=48000 || AAudioStream_getChannelCount(e->stream)!=2 || AAudioStream_getFormat(e->stream)!=AAUDIO_FORMAT_PCM_FLOAT) {
        AAudioStream_close(e->stream); delete e; return 0;
    }
    AAudioStream_setBufferSizeInFrames(e->stream,AAudioStream_getFramesPerBurst(e->stream)*2);
    e->stableAt=nowNs();
    if(AAudioStream_requestStart(e->stream)!=AAUDIO_OK) { AAudioStream_close(e->stream); delete e; return 0; }
    return reinterpret_cast<jlong>(e);
}
extern "C" JNIEXPORT void JNICALL Java_com_roomwave_receiver_NativeAudio_clock(JNIEnv*,jobject,jlong h,jlong offset,jlong at) {
    engine(h)->offset.store(offset); engine(h)->clockAt.store(at);
}
extern "C" JNIEXPORT jboolean JNICALL Java_com_roomwave_receiver_NativeAudio_push(JNIEnv* env,jobject,jlong h,jlong frame,jlong capture,jlong send,jlong play,jlong receive,jbyteArray pcm) {
    if(env->GetArrayLength(pcm)!=960 || frame<0 || frame%240!=0) return false;
    Packet p; p.frame=frame; p.capture=capture; p.send=send; p.play=play; p.received=receive;
    env->GetByteArrayRegion(pcm,0,960,reinterpret_cast<jbyte*>(p.pcm.data()));
    return engine(h)->push(p);
}
extern "C" JNIEXPORT jdoubleArray JNICALL Java_com_roomwave_receiver_NativeAudio_poll(JNIEnv* env,jobject,jlong h) {
    auto* e=engine(h); e->poll();
    const double values[]={double(e->accepted.load()),double(e->loss.load()),double(e->late.load()),double(e->rendered.load()),
        e->phase.load(),e->queueMs.load(),e->outputMs.load(),e->latencyMs.load(),e->networkMs.load(),e->captureMs.load(),
        double(AAudioStream_getXRunCount(e->stream)),double(AAudioStream_getBufferSizeInFrames(e->stream)),
        double(AAudioStream_getFramesPerBurst(e->stream)),double(AAudioStream_getPerformanceMode(e->stream)),
        double(AAudioStream_getSharingMode(e->stream)),double(e->status.load()),double(e->error.load())};
    auto result=env->NewDoubleArray(17); env->SetDoubleArrayRegion(result,0,17,values); return result;
}
extern "C" JNIEXPORT void JNICALL Java_com_roomwave_receiver_NativeAudio_close(JNIEnv*,jobject,jlong h) {
    auto* e=engine(h); AAudioStream_requestStop(e->stream); AAudioStream_close(e->stream); delete e;
}
#ifdef ROOMWAVE_TEST
#include <cassert>
#include <cstdio>
int main() {
    Engine e;
    auto now=nowNs();
    e.offset.store(0); e.clockAt.store(now); e.stampAt.store(now);
    e.stampNs.store(now+20000000); e.stampFrame.store(0);
    e.expected=24000; e.minimumFrame.store(24000);
    e.anchorFrame=24000; e.anchorPlay=now+20000000;
    // Already-dead packets must never rewind the consumer.
    Packet old; old.frame=12000; old.play=now-100000000;
    assert(!e.push(old)); assert(e.recovery.state.load()==0);
    // This is the permanent-loss state after an old host rebased time without
    // rebasing frame numbers. Consecutive future deadlines recover within a callback.
    for(int n=0;n<3;++n) {
        Packet p; p.frame=12000+n*240; p.play=now+60000000+n*5000000;
        p.send=now; p.received=now; p.capture=now; p.pcm.fill(8192);
        assert(!e.push(p));
    }
    assert(e.recovery.state.load()==2);
    std::array<float,480> out{};
    e.renderAt(out.data(),240,now,0);
    assert(e.expected==12480); assert(e.minimumFrame.load()==12480);
    for(int n=3;n<20;++n) {
        Packet p; p.frame=12000+n*240; p.play=now+60000000+n*5000000;
        p.send=now; p.received=now; p.capture=now; p.pcm.fill(8192);
        assert(e.push(p));
    }
    bool audible=false;
    for(int n=1;n<25;++n) {
        e.renderAt(out.data(),240,now+n*5000000LL,n*240);
        for(float sample:out) { assert(std::isfinite(sample)); audible |= sample>0.1f; }
    }
    assert(audible); assert(e.expected>12480);
    // A lone reordered future packet cannot reset a healthy stream.
    Engine reordered;
    reordered.clockAt.store(now); reordered.minimumFrame.store(24000);
    Packet p; p.frame=12000; p.play=now+100000000;
    assert(!reordered.push(p)); assert(reordered.recovery.state.load()==0);
    std::puts("PASS: pause timeline recovery, resumed PCM, expired packets, single-packet reorder");
}
#endif
