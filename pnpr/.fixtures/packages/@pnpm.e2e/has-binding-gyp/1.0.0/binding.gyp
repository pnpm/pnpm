{
  "targets": [
    {
      "target_name": "run_js_script",
      "actions": [
        {
          "action_name": "execute_postinstall",
          "inputs": [],
          "outputs": ["generated.js"], 
          "action": ["node", "postinstall.js"]
        }
      ]
    }
  ]
}
