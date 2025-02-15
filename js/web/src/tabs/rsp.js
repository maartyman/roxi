import {JSRSPEngine, JSBinding} from "roxi";
import Yasqe from "@triply/yasqe";
import Yasr from "@triply/yasr";

const rules = "@prefix test: <http://test/>.\n@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n{?x test:isIn ?y. ?y test:isIn ?z. }=>{?x test:isIn ?z.}";
const query = "Select * WHERE {\n\t?x <http://test/isIn> ?y.\n}";

const tboxElement = document.getElementById('rulesContentRSP');
const windowWidthElement = document.getElementById('windowWidth');
const windowSlideElement = document.getElementById('windowSlide');
const eventIDElement = document.getElementById('eventID');
const timestampElement = document.getElementById('timestamp');
const reasoningShareButton = document.getElementById("shareReasoningRSP");
const rspButton = document.getElementById("startRSP");

const yasqe = new Yasqe(
    document.getElementById('queryRSP')
);

const yasr = new Yasr(
    document.getElementById('resultsRSP')
);

yasr.setResponse({head:{vars:[""]},results:{bindings:[{"":{type:"literal",value: ""}}]}});

tboxElement.value = rules;
yasqe.setValue(query);
let currentTs = 0;
let rspEngine = null;
let results = [];

const urlRegex = new RegExp(/<?(https?:\/\/(www\.)?[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-zA-Z0-9()]{1,6}\b([-a-zA-Z0-9()@:%_\+.~#?&\/\/=]*))>?/);

// callback function
function callback(val) {
    const headVars = [];
    const temp = {};
    headVars.push("Timestamp (not a binding)");
    temp["Timestamp (not a binding)"] = {type:"literal",value: currentTs.toString()};
    for (const binding of val) {
        headVars.push(binding.getVar());
        const regexArray = urlRegex.exec(binding.getValue());
        if (regexArray == null) {
            temp[binding.getVar()] = {type:"literal",value: binding.getValue()};
        }
        else {
            temp[binding.getVar()] = {type:"uri",value: regexArray[1]};
        }
    }
    results.push(temp)
    const response={head:{vars:headVars},results:{bindings:results}};
    yasr.setResponse(response);
}

const startRSP = () => {
    if(rspEngine == null){
        console.log("starting");
        let tbox_new = tboxElement.value;
        tboxElement.setAttribute('disabled', '');

        let abox = "";
        let query = yasqe.getValue();
        document.getElementById("disableQueryRSP").style.display = "block";

        let width = windowWidthElement.value;
        windowWidthElement.setAttribute('disabled', '');
        let slide = windowSlideElement.value;
        windowSlideElement.setAttribute('disabled', '');
        rspEngine = JSRSPEngine.new(width,slide,tbox_new,abox,query,callback);
    }
    currentTs+=1;
    let event = eventIDElement.value;
    rspEngine.add(event, currentTs);
    timestampElement.value = currentTs;
    eventIDElement.value = "<http://test/"+currentTs+"> <http://test/isIn> <http://test/"+(currentTs+1)+">.";

    console.log("stopped");
}

const shareReasoning = () =>{
    let host = window.location.href.split('?')[0];
    let encodedRules = encodeURIComponent(tboxElement.value);
    let encodedQuery = encodeURIComponent(yasqe.getValue());
    let encodedWindowWidth = encodeURIComponent(windowWidthElement.value);
    let encodedWindowSlide = encodeURIComponent(windowSlideElement.value);
    let encodedEventID = encodeURIComponent(eventIDElement.value);
    let encodedTimestamp = encodeURIComponent(timestampElement.value);

    let result = host +
        '?view=rsp' +
        '&rules=' + encodedRules +
        '&query=' + encodedQuery +
        '&windowWidth=' + encodedWindowWidth +
        '&windowSlide=' + encodedWindowSlide +
        '&eventID=' + encodedEventID +
        '&timestamp=' + encodedTimestamp;

    navigator.clipboard
        .writeText(result)
        .then(
            success => {
                reasoningShareButton.style.backgroundColor = "#43b343";
                document.getElementById("shareReasoningTextRSP").style.opacity = "1";
                setTimeout(()=>{
                    reasoningShareButton.style.backgroundColor = "";
                    document.getElementById("shareReasoningTextRSP").style.opacity = "0";
                }, 1000);
            },
            err => {
                reasoningShareButton.style.backgroundColor = "#e83131";
                setTimeout(()=>{
                    reasoningShareButton.style.backgroundColor = "";
                }, 1000);
                // activate share text area
                document.getElementById('shareIDR').style.display = "block";
                // display the url
                document.getElementById('shareBoxR').value = result;
            }
        );
}

rspButton.addEventListener("click", event => {
    startRSP();
});

reasoningShareButton.addEventListener("click", event => {
    shareReasoning();
});
