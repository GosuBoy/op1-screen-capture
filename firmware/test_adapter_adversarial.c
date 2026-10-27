/* Independent native-only adversarial checks. No target addresses or devices.
 * teardown_world destroys a synthetic test fixture after assertions; it is not
 * an adapter recovery/free operation for poisoned real-world ownership.
 */
#include "adapter.h"
#include <assert.h>
#ifdef NDEBUG
#error "Independent adapter tests require active assertions"
#endif
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
typedef struct {
    op1_adapter *s;
    void *base,*raw;
    unsigned a,f;
    int fail,reenter;
}
fixture;
static void release(void *,void *);
static void reentry(fixture *m){
    uint32_t token=0xA5A5;
    assert(op1_adapter_reserve(m->s,8)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_begin_writer(m->s,&token)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_begin_hold(m->s,&token)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_begin_drain(m->s,&token)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_release_after_drain(m->s,1)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_invalidate(m->s)==OP1_ADAPTER_REENTRANT);
    assert(op1_adapter_fault(m->s)==OP1_ADAPTER_REENTRANT);
    assert(token==0xA5A5 && op1_adapter_capture_allowed(m->s)==0);
}
static void *allocate(void *ctx,uint32_t n){
    fixture*m=ctx;
    m->a++;
    if(m->reenter)reentry(m);
    if(m->fail)return NULL;
    assert(m->base==NULL);
    m->base=malloc((size_t)n+1);
    assert(m->base);
    m->raw=(unsigned char*)m->base+1;
    return m->raw;
}
static void release(void *ctx,void *p){
    fixture*m=ctx;
    assert(p==m->raw);
    assert(m->s->raw==p);
    assert(m->s->in_callback);
    if(m->reenter)reentry(m);
    free(m->base);
    m->base=NULL;
    m->raw=NULL;
    m->f++;
}
static void setup(op1_adapter*s,fixture*m){
    memset(m,0,sizeof(*m));
    m->s=s;
    assert(op1_adapter_bind(s,allocate,release,m)==OP1_ADAPTER_OK);
}
static void teardown_world(fixture*m){
    free(m->base);
    m->base=NULL;
}
static void permutations(unsigned *order,unsigned pos,unsigned *cases){
    if(pos<6){
        for(unsigned i=pos;i<6;i++){
            unsigned t=order[pos];
            order[pos]=order[i];
            order[i]=t;
            permutations(order,pos+1,cases);
            t=order[pos];
            order[pos]=order[i];
            order[i]=t;
        }
        return;
    }
    op1_adapter s=OP1_ADAPTER_INITIALIZER;
    fixture m;
    setup(&s,&m);
    assert(op1_adapter_reserve(&s,32)==OP1_ADAPTER_OK);
    uint32_t ids[6],drain;
    for(unsigned i=0;i<3;i++)assert(op1_adapter_begin_writer(&s,&ids[i])==OP1_ADAPTER_OK);
    for(unsigned i=3;i<6;i++)assert(op1_adapter_begin_hold(&s,&ids[i])==OP1_ADAPTER_OK);
    assert(op1_adapter_begin_drain(&s,&drain)==OP1_ADAPTER_OK);
    uint32_t rejected=0;
    assert(op1_adapter_begin_hold(&s,&rejected)==OP1_ADAPTER_WITHDRAWN);
    for(unsigned i=0;i<6;i++){
        assert(op1_adapter_release_after_drain(&s,drain)==OP1_ADAPTER_BUSY);
        unsigned j=order[i];
        assert((j<3?op1_adapter_end_writer(&s,ids[j]):op1_adapter_end_hold(&s,ids[j]))==OP1_ADAPTER_OK);
        assert(m.f==0);
        assert(op1_adapter_capture_allowed(&s)==0);
    }
    assert(op1_adapter_release_after_drain(&s,drain)==OP1_ADAPTER_OK);
    assert(m.a==1&&m.f==1&&s.raw==NULL);
    assert(op1_adapter_release_after_drain(&s,drain)==OP1_ADAPTER_NOT_RESERVED);
    (*cases)++;
}
static void generations(void){
    op1_adapter s=OP1_ADAPTER_INITIALIZER;
    fixture m;
    setup(&s,&m);
    uint32_t old=0;
    for(unsigned i=0;i<256;i++){
        assert(op1_adapter_reserve(&s,8)==OP1_ADAPTER_OK);
        uint32_t d;
        assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_OK);
        assert(d!=old);
        assert(op1_adapter_release_after_drain(&s,old)==OP1_ADAPTER_WRONG_DRAIN);
        assert(m.f==i);
        assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_OK);
        old=d;
    }
    assert(m.a==256&&m.f==256);
}
static void fail_and_size(void){
    op1_adapter s=OP1_ADAPTER_INITIALIZER;
    fixture m;
    setup(&s,&m);
    assert(op1_adapter_reserve(&s,0)==OP1_ADAPTER_SIZE);
    assert(op1_adapter_reserve(&s,UINT32_MAX)==OP1_ADAPTER_SIZE);
    assert(m.a==0);
    m.fail=1;
    assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_NO_MEMORY);
    assert(s.raw==NULL&&m.f==0);
    m.fail=0;
    assert(op1_adapter_reserve(&s,OP1_ADAPTER_MAX_STAGING)==OP1_ADAPTER_OK);
    uint32_t d;
    assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_OK);
    assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_OK);
    assert(m.a==2&&m.f==1);
}
static void poisoned_retains(void){
    for(unsigned kind=0;kind<5;kind++){
        op1_adapter s=OP1_ADAPTER_INITIALIZER;
        fixture m;
        setup(&s,&m);
        assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_OK);
        uint32_t d,h,w;
        assert(op1_adapter_begin_hold(&s,&h)==OP1_ADAPTER_OK);
        assert(op1_adapter_begin_writer(&s,&w)==OP1_ADAPTER_OK);
        assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_OK);
        if(kind==0)assert(op1_adapter_fault(&s)==OP1_ADAPTER_OK);
        if(kind==1)assert(op1_adapter_end_hold(&s,w)==OP1_ADAPTER_UNKNOWN_LEASE);
        if(kind==2)assert(op1_adapter_end_writer(&s,h)==OP1_ADAPTER_UNKNOWN_LEASE);
        if(kind==3)assert(op1_adapter_end_writer(&s,0)==OP1_ADAPTER_UNKNOWN_LEASE);
        if(kind==4)assert(op1_adapter_begin_writer(&s,NULL)==OP1_ADAPTER_INVALID);
        assert(op1_adapter_end_hold(&s,h)==OP1_ADAPTER_OK);
        assert(op1_adapter_end_writer(&s,w)==OP1_ADAPTER_OK);
        assert(op1_adapter_invalidate(&s)==OP1_ADAPTER_OK);
        assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_POISONED);
        assert(s.raw==m.raw&&m.f==0);
        assert(op1_adapter_capture_allowed(&s)==0);
        teardown_world(&m);
    }
}
static void late_writer(void){
    op1_adapter s=OP1_ADAPTER_INITIALIZER;
    fixture m;
    setup(&s,&m);
    assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_OK);
    uint32_t d,w;
    assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_OK);
    assert(op1_adapter_begin_writer(&s,&w)==OP1_ADAPTER_OK);
    assert(op1_adapter_invalidate(&s)==OP1_ADAPTER_OK);
    assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_BUSY);
    assert(op1_adapter_end_writer(&s,w)==OP1_ADAPTER_OK);
    assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_OK);
    assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_WITHDRAWN);
}
static void reentry_fail_closed(void){
    for(unsigned null_result=0;null_result<2;null_result++){
        op1_adapter s=OP1_ADAPTER_INITIALIZER;
        fixture m;
        setup(&s,&m);
        m.reenter=1;
        m.fail=(int)null_result;
        assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_POISONED);
        assert(s.poisoned&&s.withdrawn&&!s.in_callback&&m.a==1&&m.f==0);
        assert(s.raw==m.raw);
        assert((s.raw==NULL)==(null_result!=0));
        uint32_t d=0;
        assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_POISONED);
        assert(op1_adapter_reserve(&s,8)==OP1_ADAPTER_POISONED);
        assert(op1_adapter_capture_allowed(&s)==0);
        teardown_world(&m);
    }
    op1_adapter s=OP1_ADAPTER_INITIALIZER;
    fixture m;
    setup(&s,&m);
    assert(op1_adapter_reserve(&s,16)==OP1_ADAPTER_OK);
    uint32_t d;
    assert(op1_adapter_begin_drain(&s,&d)==OP1_ADAPTER_OK);
    m.reenter=1;
    assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_POISONED);
    assert(m.a==1&&m.f==1&&s.raw==NULL&&s.capacity==0&&s.drain_id==0&&!s.in_callback);
    assert(op1_adapter_release_after_drain(&s,d)==OP1_ADAPTER_POISONED);
    assert(m.f==1);
    assert(op1_adapter_capture_allowed(&s)==0);
}
int main(void){
    unsigned order[6]={
        0,1,2,3,4,5
    }
    ,cases=0;
    permutations(order,0,&cases);
    assert(cases==720);
    generations();
    fail_and_size();
    poisoned_retains();
    late_writer();
    reentry_fail_closed();
    puts("Independent adapter checks passed: 720 drain orders, 256 generations, reentry/failure/poison/late writer cases");
    return 0;
}
