import assert from "node:assert/strict";
import test from "node:test";
import { changeDecision } from "./call-changes.mjs";
test("Trello writes preserve external edits and allow safe replay",()=>{
  const task={editable:true,title:"Call",state:"incomplete",due:""};
  const edit={title:"Call",before:"",due:null};
  assert.equal(changeDecision(task,edit),"write");
  assert.equal(changeDecision({...task,state:"complete"},edit),"Saved");
  assert.notEqual(changeDecision({...task,due:"changed"},edit),"write");
  assert.notEqual(changeDecision(undefined,edit),"write");
  const due="2099-03-05T10:00:00Z";
  assert.equal(changeDecision(task,{...edit,due}),"write");
  assert.equal(changeDecision({...task,due},{...edit,due}),"Saved");
});
